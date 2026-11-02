//! `bern::overlay` on Linux: an undecorated, transparent window per overlay
//! with a label per line, made on the GTK thread. Locked, its input region
//! is empty, so the pointer goes through to what lies under it; unlocked, a
//! plate shows and dragging moves it. GTK 4 asks the compositor for no
//! place and no staying on top, so where it stands and whether it stays
//! above other windows is the desktop's choice.

use bern_ffi::overlay::{Host, Line, Overlay};
use gtk::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;

pub(crate) struct Overlays;

impl Host for Overlays {
    fn show(&self, id: &str, overlay: &Overlay) {
        let (id, overlay) = (id.to_string(), overlay.clone());
        glib::MainContext::default().invoke(move || show(&id, &overlay));
    }

    fn hide(&self, id: &str) {
        let id = id.to_string();
        glib::MainContext::default().invoke(move || {
            WINDOWS.with(|windows| {
                if let Some(window) = windows.borrow().get(&id) {
                    window.set_visible(false);
                }
            });
        });
    }
}

thread_local! {
    static WINDOWS: RefCell<HashMap<String, (gtk::Window, gtk::Box)>> = RefCell::new(HashMap::new());
}

const CSS: &str = "window.bern-overlay { background: transparent; }
window.bern-overlay box.bern-overlay-plate { padding: 6px 12px; border-radius: 12px; }
window.bern-overlay.unlocked box.bern-overlay-plate { background: rgba(0, 0, 0, 0.3); }
window.bern-overlay label { text-shadow: 0 1px 3px rgba(0, 0, 0, 0.85); }";

fn window(id: &str) -> (gtk::Window, gtk::Box) {
    WINDOWS.with(|windows| {
        windows
            .borrow_mut()
            .entry(id.to_string())
            .or_insert_with(|| {
                if let Some(display) = gtk::gdk::Display::default() {
                    let provider = gtk::CssProvider::new();
                    provider.load_from_string(CSS);
                    gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
                }
                let window = gtk::Window::builder().decorated(false).resizable(false).focusable(false).title("").build();
                window.add_css_class("bern-overlay");
                let plate = gtk::Box::new(gtk::Orientation::Vertical, 2);
                plate.add_css_class("bern-overlay-plate");
                window.set_child(Some(&plate));
                // Unlocked, a press starts the compositor's move.
                let press = gtk::GestureClick::new();
                let moved = window.clone();
                press.connect_pressed(move |gesture, _, x, y| {
                    if !moved.has_css_class("unlocked") {
                        return;
                    }
                    let Some(surface) = moved.surface() else { return };
                    let Ok(toplevel) = surface.downcast::<gtk::gdk::Toplevel>() else { return };
                    if let Some(device) = gesture.device() {
                        toplevel.begin_move(&device, gesture.current_button() as i32, x, y, gesture.current_event_time());
                    }
                });
                window.add_controller(press);
                (window, plate)
            })
            .clone()
    })
}

fn label(line: &Line) -> gtk::Label {
    let label = gtk::Label::new(Some(&line.text));
    let attributes = gtk::pango::AttrList::new();
    attributes.insert(gtk::pango::AttrSize::new_size_absolute((line.size * gtk::pango::SCALE as f64) as i32));
    if line.bold {
        attributes.insert(gtk::pango::AttrInt::new_weight(gtk::pango::Weight::Bold));
    }
    let [r, g, b, a] = line.color;
    let wide = |c: u8| c as u16 * 257;
    attributes.insert(gtk::pango::AttrColor::new_foreground(wide(r), wide(g), wide(b)));
    attributes.insert(gtk::pango::AttrInt::new_foreground_alpha(wide(a)));
    label.set_attributes(Some(&attributes));
    label
}

fn show(id: &str, overlay: &Overlay) {
    let (window, plate) = window(id);
    while let Some(child) = plate.first_child() {
        plate.remove(&child);
    }
    for line in overlay.lines.iter().filter(|line| !line.text.is_empty()) {
        plate.append(&label(line));
    }
    if overlay.locked {
        window.remove_css_class("unlocked");
    } else {
        window.add_css_class("unlocked");
    }
    if plate.first_child().is_none() {
        window.set_visible(false);
        return;
    }
    window.present();
    // Locked, an empty input region lets the pointer through.
    if let Some(surface) = window.surface() {
        if overlay.locked {
            surface.set_input_region(&gtk::cairo::Region::create());
        } else {
            let all = gtk::cairo::RectangleInt::new(0, 0, surface.width().max(1), surface.height().max(1));
            surface.set_input_region(&gtk::cairo::Region::create_rectangle(&all));
        }
    }
}
