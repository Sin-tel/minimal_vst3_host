use libloading::{Library, Symbol};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::ffi::c_void;
use std::sync::Arc;
use vst3::com_scrape_types::{Class, ComRef, ComWrapper};
use vst3::Steinberg::Vst::BusInfo_::BusFlags_;
use vst3::Steinberg::Vst::ProcessModes_::kRealtime;
use vst3::Steinberg::Vst::SymbolicSampleSizes_::kSample32;
use vst3::Steinberg::Vst::{
    AudioBusBuffers, AudioBusBuffers__type0, BusDirections_, BusInfo, Event__type0,
    IAudioProcessor, IAudioProcessorTrait, IComponent, IComponentTrait, IConnectionPoint,
    IConnectionPointTrait, IEditController, IEditControllerTrait, IHostApplication,
    IHostApplicationTrait, MediaTypes_, NoteOnEvent, ProcessData, ProcessSetup, SpeakerArr,
    ViewType,
};
use vst3::Steinberg::{kNotImplemented, kResultOk, tresult};
#[allow(unused_imports)]
use vst3::Steinberg::{kPlatformTypeHWND, kPlatformTypeNSView, kPlatformTypeX11EmbedWindowID};
use vst3::Steinberg::{
    IPlugFrame, IPlugFrameTrait, IPlugView, IPlugViewTrait, IPluginBaseTrait, IPluginFactory,
    IPluginFactoryTrait, PClassInfo, ViewRect,
};
use vst3::{ComPtr, Interface};
use winit::window::Window;

fn extract_cstring(bytes: &[i8]) -> String {
    let len = bytes.iter().position(|&c| c == 0).unwrap_or(bytes.len());
    let u8_bytes: Vec<u8> = bytes[..len].iter().map(|&b| b as u8).collect();
    String::from_utf8_lossy(&u8_bytes).to_string()
}

fn extract_cstring_utf16(bytes: &[u16]) -> String {
    let len = bytes.iter().position(|&c| c == 0).unwrap_or(bytes.len());
    let u16_str: Vec<u16> = bytes[..len].iter().map(|&b| b as u16).collect();
    String::from_utf16_lossy(&u16_str).to_string()
}

pub const BUF_SIZE: usize = 512;
const SAMPLE_RATE: f64 = 44100.0;

struct PluginHost;

impl Class for PluginHost {
    type Interfaces = (IHostApplication,);
}

impl IHostApplicationTrait for PluginHost {
    unsafe fn getName(&self, _name: *mut [u16; 128]) -> tresult {
        // TODO: write something to name
        kResultOk
    }

    unsafe fn createInstance(
        &self,
        _cid: *mut [i8; 16],
        _iid: *mut [i8; 16],
        _obj: *mut *mut std::ffi::c_void,
    ) -> tresult {
        // TODO
        kNotImplemented
    }
}

pub struct PluginFrame;

impl Class for PluginFrame {
    type Interfaces = (IPlugFrame,);
}

impl IPlugFrameTrait for PluginFrame {
    unsafe fn resizeView(
        &self,
        _view: *mut vst3::Steinberg::IPlugView,
        _new_size: *mut ViewRect,
    ) -> tresult {
        // TODO
        kResultOk
    }
}

use vst3::Steinberg::Vst::{Event, IEventList, IEventListTrait};

struct EventList {
    events: Vec<Event>,
}

impl Class for EventList {
    type Interfaces = (IEventList,);
}

impl IEventListTrait for EventList {
    unsafe fn getEventCount(&self) -> i32 {
        self.events.len() as i32
    }

    unsafe fn getEvent(&self, index: i32, event: *mut Event) -> tresult {
        if index >= 0 && index < self.events.len() as i32 {
            unsafe { *event = self.events[index as usize] };
            kResultOk
        } else {
            vst3::Steinberg::kResultFalse
        }
    }

    unsafe fn addEvent(&self, _event: *mut Event) -> tresult {
        // The plugin doesn't call this on our input list, so we can ignore it
        kResultOk
    }
}

type GetPluginFactoryFunc = unsafe extern "system" fn() -> *mut vst3::Steinberg::FUnknown;

pub struct Vst3Library {
    lib: Library,
}

impl Vst3Library {
    pub fn new(path: &str) -> Result<Arc<Self>, String> {
        let lib = unsafe { Library::new(path).map_err(|e| e.to_string())? };

        unsafe {
            if let Ok(init_dll) = lib.get::<unsafe extern "system" fn() -> bool>(c"InitDll") {
                init_dll();
            }
        }

        Ok(Arc::new(Self { lib }))
    }

    pub fn get_factory(&self) -> Result<*mut vst3::Steinberg::FUnknown, String> {
        let get_factory: Symbol<GetPluginFactoryFunc> = unsafe {
            self.lib
                .get(c"GetPluginFactory")
                .map_err(|e| e.to_string())?
        };
        Ok(unsafe { get_factory() })
    }
}

impl Drop for Vst3Library {
    fn drop(&mut self) {
        unsafe {
            if let Ok(exit_dll) = self
                .lib
                .get::<unsafe extern "system" fn() -> bool>(c"ExitDll")
            {
                exit_dll();
            }
        }
    }
}

#[allow(unused)]
pub struct Vst3Editor {
    plug_view: Option<ComPtr<IPlugView>>,
    edit_controller: ComPtr<IEditController>,
    host_context: ComWrapper<PluginHost>,
    lib: Arc<Vst3Library>,
}

#[allow(unused)]
pub struct Vst3Processor {
    audio_processor: ComPtr<IAudioProcessor>,
    component: ComPtr<IComponent>,
    lib: Arc<Vst3Library>,
}

pub fn load(path: &str) -> Result<(Vst3Editor, Vst3Processor), String> {
    let lib = Vst3Library::new(path)?;

    // Get the factory
    let factory_ptr = lib.get_factory()?;

    let factory = unsafe { ComRef::<IPluginFactory>::from_raw(factory_ptr as *mut _).unwrap() };

    let class_count = unsafe { factory.countClasses() };
    println!("Found {} classes in the VST3 bundle.", class_count);

    let mut processor_cid: Option<[i8; 16]> = None;
    let mut editor_cid: Option<[i8; 16]> = None;

    for i in 0..class_count {
        // Zero-initialize the struct that the factory will fill out
        let mut class_info: PClassInfo = unsafe { std::mem::zeroed() };

        let res = unsafe { factory.getClassInfo(i, &mut class_info) };

        if res == kResultOk {
            let name = extract_cstring(&class_info.name);
            let category = extract_cstring(&class_info.category);
            println!("Class {}: '{}' ({})", i, name, category);

            if category == "Audio Module Class" {
                processor_cid = Some(class_info.cid);
            } else if category == "Component Controller Class" {
                editor_cid = Some(class_info.cid);
            }
        }
    }

    let processor_id = processor_cid.expect("Could not find an Audio Module Class");
    let editor_id = editor_cid.expect("Could not find an Component Controller Class");

    // Create host context
    let host_context = ComWrapper::new(PluginHost);
    let host_ptr = host_context.to_com_ptr::<IHostApplication>().unwrap();

    // Create the processor instance
    let mut component_ptr: *mut c_void = std::ptr::null_mut();
    unsafe {
        factory.createInstance(
            processor_id.as_ptr(),
            IComponent::IID.as_ptr() as *const i8,
            &mut component_ptr,
        );
    }
    let component = unsafe { ComPtr::from_raw(component_ptr as *mut IComponent).unwrap() };

    // Initialize the plugin
    let res = unsafe { component.initialize(host_ptr.as_ptr() as *mut vst3::Steinberg::FUnknown) };
    assert_eq!(res, kResultOk);

    // Query the IAudioProcessor interface
    let audio_processor = component
        .cast::<IAudioProcessor>()
        .expect("Component does not implement IAudioProcessor");

    // Tell it about audio engine settings
    let mut setup = ProcessSetup {
        processMode: kRealtime,
        symbolicSampleSize: kSample32,
        maxSamplesPerBlock: BUF_SIZE as i32,
        sampleRate: SAMPLE_RATE,
    };

    let res = unsafe { audio_processor.setupProcessing(&mut setup) };
    assert_eq!(res, kResultOk);

    let res = unsafe {
        audio_processor.setBusArrangements(
            // input
            std::ptr::null_mut(),
            0,
            // output
            &SpeakerArr::kStereo as *const _ as *mut _,
            1,
        )
    };
    if res != kResultOk {
        println!("Default stereo bus arrangement not accepted.");
        let bus_count =
            unsafe { component.getBusCount(MediaTypes_::kAudio, BusDirections_::kOutput) };

        println!("Output bus count: {:?}", bus_count);

        for i in 0..bus_count {
            let mut bus_info: BusInfo = unsafe { std::mem::zeroed() };
            let res = unsafe {
                component.getBusInfo(
                    MediaTypes_::kAudio,
                    BusDirections_::kOutput,
                    i,
                    &mut bus_info,
                )
            };
            assert_eq!(res, kResultOk);

            println!(
                "bus: {i} name: {:?} channelCount: {:?} default: {:?}",
                extract_cstring_utf16(&bus_info.name),
                bus_info.channelCount,
                bus_info.flags & BusFlags_::kDefaultActive as u32 > 0,
            );
        }
    }

    // Activate bus 0
    let res = unsafe { component.activateBus(MediaTypes_::kAudio, BusDirections_::kOutput, 0, 1) };
    assert_eq!(res, kResultOk);

    let res = unsafe { component.setActive(1) };
    assert_eq!(res, kResultOk);

    let res = unsafe { audio_processor.setProcessing(1) };
    assert_eq!(res, kResultOk);

    // This may work for some plugins.
    // let editor = component.cast::<IEditController>().unwrap_or_else(|| {
    //     panic!("Processor does not implement IEditController directly.");
    // });

    // Create the editor instance
    let mut editor_ptr: *mut c_void = std::ptr::null_mut();
    unsafe {
        factory.createInstance(
            editor_id.as_ptr(),
            IEditController::IID.as_ptr() as *const i8,
            &mut editor_ptr,
        );
    }
    let edit_controller = unsafe { ComPtr::from_raw(editor_ptr as *mut IEditController).unwrap() };

    let res = unsafe {
        // Some plugins require the editor to be initialized with the host context too
        edit_controller.initialize(host_ptr.as_ptr() as *mut vst3::Steinberg::FUnknown)
    };
    assert_eq!(res, kResultOk);

    // Attempt to cast both to IConnectionPoint
    // Should only be necessary if they are seperate components
    let audio_connection = audio_processor.cast::<IConnectionPoint>();
    let edit_connection = edit_controller.cast::<IConnectionPoint>();

    if let (Some(c1), Some(c2)) = (audio_connection, edit_connection) {
        unsafe {
            let res1 = c1.connect(c2.as_ptr() as *mut IConnectionPoint);
            let res2 = c2.connect(c1.as_ptr() as *mut IConnectionPoint);
            assert_eq!(res1, kResultOk);
            assert_eq!(res2, kResultOk);
        }
    } else {
        return Err("Plugin does not support IConnectionPoint".into());
    }

    let editor = Vst3Editor {
        plug_view: None,
        edit_controller,
        host_context,
        lib: Arc::clone(&lib),
    };
    let processor = Vst3Processor {
        audio_processor,
        component,
        lib: Arc::clone(&lib),
    };

    Ok((editor, processor))
}

impl Vst3Editor {
    pub fn open_window(&mut self, window: &Window) -> Result<(), String> {
        let view_ptr = unsafe { self.edit_controller.createView(ViewType::kEditor) };
        if view_ptr.is_null() {
            return Err("Plugin does not have a GUI!".into());
        }

        let plug_view = unsafe { ComPtr::from_raw(view_ptr as *mut IPlugView).unwrap() };

        let raw_window_handle = window.window_handle().ok().map(|wh| wh.as_raw()).unwrap();

        // Get platform specific handle
        let (system_window_handle, platform_type) = match raw_window_handle {
            #[cfg(target_os = "windows")]
            RawWindowHandle::Win32(handle) => (handle.hwnd.get() as *mut c_void, kPlatformTypeHWND),
            #[cfg(target_os = "macos")]
            RawWindowHandle::AppKit(handle) => {
                (handle.ns_view.as_ptr() as *mut c_void, kPlatformTypeNSView)
            }
            #[cfg(target_os = "linux")]
            RawWindowHandle::Xlib(handle) => {
                (handle.window as *mut c_void, kPlatformTypeX11EmbedWindowID)
            }
            _ => return Err("Unsupported platform.".into()),
        };

        let res = unsafe { plug_view.attached(system_window_handle, platform_type) };
        assert_eq!(res, kResultOk);

        let frame_obj = ComWrapper::new(PluginFrame);

        // TODO: frame is dropped when it goes out of scope
        let frame_ptr = frame_obj.to_com_ptr::<IPlugFrame>().unwrap();

        let res = unsafe { plug_view.setFrame(frame_ptr.as_ptr() as *mut IPlugFrame) };
        assert_eq!(res, kResultOk);

        let mut view_rect = vst3::Steinberg::ViewRect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        unsafe {
            if plug_view.getSize(&mut view_rect) == kResultOk {
                let width = (view_rect.right - view_rect.left) as f64;
                let height = (view_rect.bottom - view_rect.top) as f64;
                let _ = window.request_inner_size(winit::dpi::LogicalSize::new(width, height));
            }
        }

        self.plug_view = Some(plug_view);
        Ok(())
    }
}

impl Vst3Processor {
    pub fn process(&self, left_buf: &mut [f32], right_buf: &mut [f32]) {
        // let mut note_on: Event = unsafe { std::mem::zeroed() };
        // note_on.__field0 = Event__type0 {
        //     noteOn: NoteOnEvent {
        //         channel: 0,
        //         pitch: 60,
        //         tuning: 0.0,
        //         velocity: 0.8,
        //         length: 0,
        //         noteId: 1,
        //     },
        // };

        let event_list_obj = ComWrapper::new(EventList {
            // events: vec![note_on],
            events: vec![],
        });
        let event_list_ptr = event_list_obj.to_com_ptr::<IEventList>().unwrap();

        // VST3 wants a pointer to an array of channel pointers
        let mut channels = [left_buf.as_mut_ptr(), right_buf.as_mut_ptr()];

        let mut output_bus = AudioBusBuffers {
            numChannels: 2,
            silenceFlags: 0,
            __field0: AudioBusBuffers__type0 {
                channelBuffers32: channels.as_mut_ptr(),
            },
        };

        // Populate buffer process data
        let mut process_data: ProcessData = unsafe { std::mem::zeroed() };
        process_data.processMode = kRealtime;
        process_data.symbolicSampleSize = kSample32;
        process_data.numSamples = BUF_SIZE as i32;

        // Output wiring
        process_data.numOutputs = 1; // 1 stereo bus
        process_data.outputs = &mut output_bus;

        // Input Events wiring
        process_data.inputEvents = event_list_ptr.as_ptr() as *mut IEventList;

        // Run processing
        let res = unsafe { self.audio_processor.process(&mut process_data) };
        assert_eq!(res, kResultOk);
    }
}
