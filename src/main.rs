use crate::vst::Vst3Plugin;
use crate::vst::BUF_SIZE;
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

        let mut plugin = Vst3Plugin::load(PATH).unwrap();

        let _ = plugin.open_window(&window);

        std::thread::spawn(move || {
            // Fake audio thread
            let mut left_buf = [0.; BUF_SIZE];
            let mut right_buf = [0.; BUF_SIZE];

            loop {
                left_buf.fill(0.);
                right_buf.fill(0.);
                plugin.process(&mut left_buf, &mut right_buf);

                // check if we wrote anything to the buffer
                let mut sum = 0.0;
                for s in left_buf.iter() {
                    sum += s.abs();
                }
                if sum > 0.1 {
                    println!("Sum of absolute audio output: {}", sum);
                }

                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        });

        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
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
