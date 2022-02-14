use i4config::keyboard::mods::{ALT, CTRL, Modifiers, SHIFT};
use i4config::keyboard::syms::{SYM_Super_L, SYM_h, SYM_j, SYM_k, SYM_l, SYM_plus, SYM_minus, SYM_r};
use i4config::Direction::{Down, Left, Right, Up};
use i4config::{config, shell, Seat, create_seat, input_devices, on_new_input_device};

const MOD: Modifiers = ALT;

fn configure_seat(s: Seat) {
    log::info!("Configuring seat {:?}", s);

    let change_rate = move |delta| {
        let (rate, delay) = s.repeat_rate();
        let new_rate = rate - delta;
        let new_delay = delay + 10 * delta;
        s.set_repeat_rate(new_rate, new_delay);
    };

    s.bind(CTRL | SYM_l, move || change_rate(-1));
    s.bind(CTRL | SYM_r, move || change_rate(1));

    s.bind(CTRL | SYM_h, move || s.focus(Left));
    s.bind(CTRL | SYM_j, move || s.focus(Down));
    s.bind(CTRL | SYM_k, move || s.focus(Up));
    s.bind(CTRL | SYM_l, move || s.focus(Right));

    s.bind(MOD | SHIFT | SYM_h, move || s.move_(Left));
    s.bind(MOD | SHIFT | SYM_j, move || s.move_(Down));
    s.bind(MOD | SHIFT | SYM_k, move || s.move_(Up));
    s.bind(MOD | SHIFT | SYM_l, move || s.move_(Right));

    s.bind(SYM_Super_L, || shell("alacritty"));
}

pub fn configure() {
    let seat = create_seat("default");
    configure_seat(seat);
    for device in input_devices() {
        device.set_seat(seat);
    }
    on_new_input_device(move |device| device.set_seat(seat));
}

config!(configure);