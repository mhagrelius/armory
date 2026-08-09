use gtk::prelude::*;

use armory::ui::ArmoryApplication;

fn main() -> gtk::glib::ExitCode {
    gtk::glib::set_application_name("Armory");
    gtk::glib::set_prgname(Some(armory::APP_ID));
    ArmoryApplication::new().run()
}
