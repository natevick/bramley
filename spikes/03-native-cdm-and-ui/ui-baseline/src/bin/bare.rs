use std::time::Instant;

struct App {
    started: Instant,
    logged: bool,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        ui_baseline::report_frame(self.started, &mut self.logged, frame);
        ui.heading("bare eframe glow window");
        ui.label("idle");
    }
}

fn main() -> eframe::Result {
    let started = Instant::now();
    eframe::run_native("spike3a-bare", ui_baseline::options(), Box::new(move |_cc| Ok(Box::new(App { started, logged: false }))))
}
