use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

pub static FRAMES: AtomicU64 = AtomicU64::new(0);

fn gl_strings(gl: &eframe::glow::Context) -> (String, String) {
    use eframe::glow::HasContext;
    unsafe { (gl.get_parameter_string(eframe::glow::RENDERER), gl.get_parameter_string(eframe::glow::VERSION)) }
}

pub fn report_frame(started: Instant, logged_renderer: &mut bool, frame: &eframe::Frame) {
    let n = FRAMES.fetch_add(1, Ordering::Relaxed) + 1;
    if !*logged_renderer {
        match frame.gl() {
            Some(gl) => {
                let (renderer, version) = gl_strings(gl);
                eprintln!("SPIKE gl_renderer={renderer:?} gl_version={version:?}");
            }
            None => eprintln!("SPIKE gl_renderer=none"),
        }
        *logged_renderer = true;
    }
    if n == 1 || n % 50 == 0 {
        eprintln!("SPIKE frames={n} t_ms={}", started.elapsed().as_millis());
    }
}

pub fn options() -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1100.0, 720.0]).with_title("spike3a"),
        ..Default::default()
    }
}
