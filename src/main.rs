use crate::vst::BUF_SIZE;
use crate::vst::Vst3Editor;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

mod error;
mod event;
mod scan;
mod util;
mod vst;

const PLUGIN_NAME: &str = "surge";

#[derive(Default)]
struct App {
    window: Option<Window>,
    editor: Option<Vst3Editor>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = event_loop
            .create_window(Window::default_attributes())
            .unwrap();

        let mut all_plugins = Vec::new();
        for path in scan::standard_vst3_paths() {
            all_plugins.extend(scan::scan_folder(&path));
        }

        println!("Found {} plugins.", all_plugins.len());

        let mut to_load = None;
        for p in &all_plugins {
            if p.is_instrument
                && p.name
                    .to_ascii_lowercase()
                    .contains(&PLUGIN_NAME.to_ascii_lowercase())
            {
                to_load = Some(p);
                break;
            }
        }

        if to_load.is_none() {
            println!("No plugin matching \"{PLUGIN_NAME}\" found.");
        }

        if let Some(plugin) = to_load {
            println!("Loading: {:?}", plugin.name);
            let (mut editor, mut processor) = vst::load(&plugin.library_path).unwrap();

            let _ = editor.open_window(&window);

            std::thread::spawn(move || {
                // Fake audio thread
                let mut left_buf = [0.; BUF_SIZE];
                let mut right_buf = [0.; BUF_SIZE];

                let mut counter = 0;

                processor.events.push(event::note_on(0, 60, 0.0, 0.8));

                loop {
                    left_buf.fill(0.);
                    right_buf.fill(0.);

                    if counter == 100 {
                        processor.events.push(event::note_off(0, 60, 0.0));
                    }
                    counter += 1;

                    processor.process(&mut left_buf, &mut right_buf);

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
            // keep alive
            self.editor = Some(editor);
        }

        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                // do not call request_redraw here!
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
