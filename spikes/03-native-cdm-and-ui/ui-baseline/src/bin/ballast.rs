use ui_baseline::variant::{self, Parts};

fn main() -> eframe::Result {
    variant::run("ballast", Parts { ballast_mib: 20, ..variant::BARELIST })
}
