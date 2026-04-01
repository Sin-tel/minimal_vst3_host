use libloading::{Library, Symbol};
use std::ffi::c_void;
use vst3::com_scrape_types::{Class, ComRef, ComWrapper};
use vst3::Steinberg::Vst::ProcessModes_::kRealtime;
use vst3::Steinberg::Vst::SymbolicSampleSizes_::kSample32;
use vst3::Steinberg::Vst::{
    AudioBusBuffers, AudioBusBuffers__type0, Event__type0, IAudioProcessor, IAudioProcessorTrait,
    IComponent, IComponentTrait, IConnectionPoint, IConnectionPointTrait, IEditController,
    IEditControllerTrait, IHostApplication, IHostApplicationTrait, NoteOnEvent, ProcessData,
    ProcessSetup, ViewType,
};
use vst3::Steinberg::{kNotImplemented, kResultOk, tresult};
use vst3::Steinberg::{
    IPlugFrame, IPlugFrameTrait, IPlugView, IPluginBaseTrait, IPluginFactory, IPluginFactoryTrait,
    PClassInfo, ViewRect,
};
use vst3::{ComPtr, Interface};

// TODO: snake_case!

fn extract_cstring(bytes: &[i8]) -> String {
    let len = bytes.iter().position(|&c| c == 0).unwrap_or(bytes.len());
    let u8_bytes: Vec<u8> = bytes[..len].iter().map(|&b| b as u8).collect();
    String::from_utf8_lossy(&u8_bytes).to_string()
}

const BUF_SIZE: usize = 512;

struct PluginHost;

impl Class for PluginHost {
    // We tell COM that this object implements IHostApplication
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

struct MockEventList {
    events: Vec<Event>,
}

impl Class for MockEventList {
    type Interfaces = (IEventList,);
}

impl IEventListTrait for MockEventList {
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

pub fn init_plugin(path: &str) -> (Library, ComPtr<IAudioProcessor>, ComPtr<IEditController>) {
    let lib = unsafe { Library::new(path).unwrap() };

    // Get the factory
    let get_factory: Symbol<GetPluginFactoryFunc> =
        unsafe { lib.get(b"GetPluginFactory\0").unwrap() };
    let factory_ptr = unsafe { get_factory() };

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
    let host_obj = ComWrapper::new(PluginHost);
    let host_ptr = host_obj.to_com_ptr::<IHostApplication>().unwrap();

    // Create the processor instance
    let mut processor_ptr: *mut c_void = std::ptr::null_mut();
    unsafe {
        factory.createInstance(
            processor_id.as_ptr(),
            IComponent::IID.as_ptr() as *const i8,
            &mut processor_ptr,
        );
    }
    let processor_component =
        unsafe { ComPtr::from_raw(processor_ptr as *mut IComponent).unwrap() };

    // Initialize the plugin
    let res = unsafe {
        processor_component.initialize(host_ptr.as_ptr() as *mut vst3::Steinberg::FUnknown)
    };
    assert_eq!(res, kResultOk);

    // Query the IAudioProcessor interface
    let processor = processor_component
        .cast::<IAudioProcessor>()
        .expect("Component does not implement IAudioProcessor");

    // Tell it about audio engine settings
    let mut setup = ProcessSetup {
        processMode: kRealtime,
        symbolicSampleSize: kSample32,
        maxSamplesPerBlock: BUF_SIZE as i32,
        sampleRate: 44100.0,
    };

    let res = unsafe { processor.setupProcessing(&mut setup) };
    assert_eq!(res, kResultOk);

    let res = unsafe { processor_component.setActive(1) };
    assert_eq!(res, kResultOk);

    let res = unsafe { processor.setProcessing(1) };
    assert_eq!(res, kResultOk);

    // Create the editor instance
    let mut editor_ptr: *mut c_void = std::ptr::null_mut();
    unsafe {
        factory.createInstance(
            editor_id.as_ptr(),
            IEditController::IID.as_ptr() as *const i8,
            &mut editor_ptr,
        );
    }
    let editor = unsafe { ComPtr::from_raw(editor_ptr as *mut IEditController).unwrap() };

    // This may work for some plugins.

    // let editor = processor_component
    //     .cast::<IEditController>()
    //     .unwrap_or_else(|| {
    //         // If it panics here, it truly is a Distributed Component and we will
    //         // need to implement `IComponentHandler` to link them.
    //         panic!("Processor does not implement IEditController directly!");
    //     });

    let res = unsafe {
        // Some plugins require the editor to be initialized with the host context too
        editor.initialize(host_ptr.as_ptr() as *mut vst3::Steinberg::FUnknown)
    };
    assert_eq!(res, kResultOk);

    // Attempt to cast both to IConnectionPoint
    // Should only be necessary if they are seperate components
    let comp_connection = processor_component.cast::<IConnectionPoint>();
    let edit_connection = editor.cast::<IConnectionPoint>();

    if let (Some(cp_comp), Some(cp_edit)) = (comp_connection, edit_connection) {
        unsafe {
            // Connect the processor to the editor
            let res1 = cp_comp.connect(cp_edit.as_ptr() as *mut IConnectionPoint);
            // Connect the editor to the processor
            let res2 = cp_edit.connect(cp_comp.as_ptr() as *mut IConnectionPoint);

            std::mem::forget(cp_comp);
            std::mem::forget(cp_edit);
            if res1 == kResultOk && res2 == kResultOk {
                println!("Successfully wired Processor and Editor together!");
            }
        }
    } else {
        println!("Plugin does not support IConnectionPoint (unexpected for separated components)");
    }

    std::mem::forget(processor_component);

    (lib, processor, editor)
}

pub fn get_view(editor: &ComPtr<IEditController>) -> ComPtr<IPlugView> {
    let view_ptr = unsafe { editor.createView(ViewType::kEditor) };
    if view_ptr.is_null() {
        panic!("Plugin does not have a GUI!");
    }

    let plug_view = unsafe { ComPtr::from_raw(view_ptr as *mut IPlugView).unwrap() };

    plug_view
}

pub fn fake_process(audio_processor: &ComPtr<IAudioProcessor>) {
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

    let event_list_obj = ComWrapper::new(MockEventList {
        // events: vec![note_on],
        events: vec![],
    });
    let event_list_ptr = event_list_obj.to_com_ptr::<IEventList>().unwrap();

    // Create buffers
    let mut left_buf = vec![0.0f32; BUF_SIZE];
    let mut right_buf = vec![0.0f32; BUF_SIZE];

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
    let res = unsafe { audio_processor.process(&mut process_data) };
    assert_eq!(res, kResultOk);

    // check if we wrote anything to the buffer
    // let mut sum = 0.0;
    // for s in left_buf.iter() {
    //     sum += s.abs();
    // }
    // println!("Sum of absolute audio output: {}", sum);
}
