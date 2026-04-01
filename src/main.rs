use crate::vst::PluginFrame;
use raw_window_handle::HasWindowHandle;
use raw_window_handle::RawWindowHandle;
use std::ffi::c_void;
use vst3::ComWrapper;
use vst3::Steinberg::{kPlatformTypeHWND, kResultOk, IPlugFrame, IPlugViewTrait};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

mod vst;

const PATH: &str = r"C:\Program Files\Common Files\VST3\Pianoteq 7.vst3";

#[derive(Default)]
struct App {
    window: Option<Window>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = event_loop
            .create_window(Window::default_attributes())
            .unwrap();

        let raw_window_handle = window.window_handle().ok().map(|wh| wh.as_raw()).unwrap();

        let (system_window_handle, platform_type) = match raw_window_handle {
            RawWindowHandle::Win32(handle) => (handle.hwnd.get() as *mut c_void, kPlatformTypeHWND),
            _ => panic!("Unsupported platform."),
        };

        let (lib, processor, editor) = vst::init_plugin(PATH);
        std::mem::forget(lib);

        let plugin_view = vst::get_view(&editor);

        let frame_obj = ComWrapper::new(PluginFrame);
        let frame_ptr = frame_obj.to_com_ptr::<IPlugFrame>().unwrap();

        std::mem::forget(frame_obj);

        let res = unsafe { plugin_view.setFrame(frame_ptr.as_ptr() as *mut IPlugFrame) };
        assert_eq!(res, kResultOk);

        let res = unsafe { plugin_view.attached(system_window_handle, platform_type) };
        assert_eq!(res, kResultOk);

        let mut view_rect = vst3::Steinberg::ViewRect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        unsafe {
            if plugin_view.getSize(&mut view_rect) == kResultOk {
                let width = (view_rect.right - view_rect.left) as f64;
                let height = (view_rect.bottom - view_rect.top) as f64;
                let _ = window.request_inner_size(winit::dpi::LogicalSize::new(width, height));
            }
        }

        std::thread::spawn(move || loop {
            vst::fake_process(&processor);
            std::thread::sleep(std::time::Duration::from_millis(10));
        });

        std::mem::forget(editor);
        std::mem::forget(plugin_view);

        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                println!("The close button was pressed; stopping");
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                self.window.as_ref().unwrap().request_redraw();
            }
            _ => (),
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().unwrap();

    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App::default();
    let _ = event_loop.run_app(&mut app);
}
