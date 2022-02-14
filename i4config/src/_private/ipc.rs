use crate::keyboard::mods::Modifiers;
use crate::keyboard::syms::KeySym;
use crate::{Direction, InputDevice, LogLevel, Seat};
use bincode::{BorrowDecode, Decode, Encode};
use crate::keyboard::keymap::Keymap;

#[derive(Encode, BorrowDecode, Debug)]
pub enum Request<'a> {
    Configure,
    Log {
        level: LogLevel,
        msg: &'a str,
        file: Option<&'a str>,
        line: Option<u32>,
    },
    Response {
        response: Response,
    },
    CreateSeat {
        name: &'a str,
    },
    SetSeat {
        device: InputDevice,
        seat: Seat,
    },
    ParseKeymap {
        keymap: &'a str,
    },
    SeatSetKeymap {
        seat: Seat,
        keymap: Keymap,
    },
    SeatGetRepeatRate {
        seat: Seat,
    },
    SeatSetRepeatRate {
        seat: Seat,
        rate: i32,
        delay: i32,
    },
    RemoveSeat {
        seat: Seat,
    },
    GetSeats,
    GetInputDevices,
    NewInputDevice {
        device: InputDevice,
    },
    DelInputDevice {
        device: InputDevice,
    },
    AddShortcut {
        seat: Seat,
        mods: Modifiers,
        sym: KeySym,
    },
    RemoveShortcut {
        seat: Seat,
        mods: Modifiers,
        sym: KeySym,
    },
    InvokeShortcut {
        seat: Seat,
        mods: Modifiers,
        sym: KeySym,
    },
    Shell {
        script: &'a str,
    },
    Focus {
        seat: Seat,
        direction: Direction,
    },
    Move {
        seat: Seat,
        direction: Direction,
    },
}

#[derive(Encode, Decode, Debug)]
pub enum Response {
    None,
    GetSeats { seats: Vec<Seat> },
    GetRepeatRate { rate: i32, delay: i32 },
    ParseKeymap { keymap: Keymap, },
    CreateSeat { seat: Seat },
    GetInputDevices { devices: Vec<InputDevice> },
}

#[derive(Encode, Decode, Debug)]
pub enum InitMessage {
    V1(V1InitMessage),
}

#[derive(Encode, Decode, Debug)]
pub struct V1InitMessage {}