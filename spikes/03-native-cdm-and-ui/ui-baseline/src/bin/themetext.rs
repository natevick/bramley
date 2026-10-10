use ui_baseline::variant::{self, Parts};

fn main() -> eframe::Result {
    variant::run("themetext", Parts { theme_text: true, ..variant::BARELIST })
}
