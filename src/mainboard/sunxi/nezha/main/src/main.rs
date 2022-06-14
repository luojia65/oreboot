#![no_std]
#![no_main]
#![feature(default_alloc_error_handler)]
#![feature(naked_functions)]
#![feature(asm_sym, asm_const)]

// use core::arch::global_asm;
// use core::fmt::Write;
use core::arch::asm;
use core::panic::PanicInfo;
// use oreboot_arch::riscv64 as arch;
// use oreboot_drivers::{
//     uart::sunxi::{Sunxi, UART0},
//     wrappers::{DoD, Memory, SectionReader},
//     Driver,
// };
// use oreboot_soc::sunxi::d1::{ccu::CCU, gpio::GPIO};
// use payloads::payload;
// use sbi::sbi_init;
use embedded_hal::digital::blocking::OutputPin;
use oreboot_soc::sunxi::d1::{
    ccu::Clocks,
    gpio::Gpio,
    pac::Peripherals,
    uart::{Config, Parity, Serial, StopBits, WordLength},
    time::U32Ext,
};
#[macro_use]
mod logging;

// when handled from BT0 stage, DDR is prepared.
// this code runs from DDR start
#[naked]
#[export_name = "_start"]
#[link_section = ".text.entry"]
unsafe extern "C" fn start() -> ! {
    asm!(
        // 1. clear cache and processor states
        // BT0 stage already handled for us
        // 2. initialize programming langauge runtime
        // clear bss segment
        "la     t0, sbss",
        "la     t1, ebss",
        "1:",
        "bgeu   t0, t1, 1f",
        "sd     x0, 0(t0)",
        "addi   t0, t0, 4",
        "j      1b",
        "1:", 
        // 3. prepare stack
        "la     sp, {stack}",
        "li     t0, {stack_size}",
        "add    sp, sp, t0",
        "call   {main}",
        // Function `main` returns with hardware power operation type
        // which may be reboot or shutdown. Function `finish` would
        // perform these operations.
        "j      {finish}",
        stack      =   sym ENV_STACK,
        stack_size = const STACK_SIZE,
        main       =   sym main,
        finish     =   sym finish,
        options(noreturn)
    )
}

// stack which the bootloader environment would make use of.
#[link_section = ".bss.uninit"]
static mut ENV_STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];
const STACK_SIZE: usize = 1 * 1024; // 1KiB

// Function `main`. It would initialize an environment for the kernel.
// The environment does not exit when bootloading stage is finished;
// it remains in background to provide environment features which the
// kernel would make use of.
// Those features would include RISC-V SBI calls, instruction emulations,
// misaligned and so on.
#[no_mangle]
extern "C" fn main() -> usize {
    let p = Peripherals::take().unwrap();
    let clocks = Clocks {
        psi: 600_000_000.hz(),
        apb1: 24_000_000.hz(),
    };
    let gpio = Gpio::new(p.GPIO);
    // turn off led
    let mut pb5 = gpio.portb.pb5.into_output();
    pb5.set_low().unwrap();

    // prepare serial port logger
    let tx = gpio.portb.pb8.into_function_6();
    let rx = gpio.portb.pb9.into_function_6();
    let config = Config {
        baudrate: 115200.bps(),
        wordlength: WordLength::Eight,
        parity: Parity::None,
        stopbits: StopBits::One,
    };
    let serial = Serial::new(p.UART0, (tx, rx), config, &clocks);
    crate::logging::set_logger(serial);

    println!("!oreboot from DDR 🦀").ok();
    // // clock
    // let mut ccu = CCU::new();
    // ccu.init().unwrap();
    // let mut gpio = GPIO::new();
    // gpio.init().unwrap();
    // let mut uart0 = Sunxi::new(UART0 as usize, 115200);
    // uart0.init().unwrap();
    // uart0.pwrite(b"UART0 initialized\r\n", 0).unwrap();

    // let mut uarts = [&mut uart0 as &mut dyn Driver];
    // let console = &mut DoD::new(&mut uarts[..]);
    // console.init().unwrap();
    // console.pwrite(b"Welcome to oreboot\r\n", 0).unwrap();

    // let w = &mut print::WriteTo::new(console);
    // writeln!(w, "## Loading payload\r").unwrap();

    // // see ../fixed-dtfs.dts
    // // TODO: adjust when DRAM driver is implemented / booting from SPI
    // let mem = 0x4000_0000;
    // let cached_mem = 0x8000_0000;
    // let payload_offset = 0x2_0000;
    // let payload_size = 0x1e_0000;
    // let linuxboot_offset = 0x20_0000;
    // let linuxboot_size = 0x120_0000;
    // let dtb_offset = 0x140_0000;
    // let dtb_size = 0xe000;

    // // TODO; This payload structure should be loaded from boot medium rather
    // // than hardcoded.
    // let segs = &[
    //     payload::Segment {
    //         typ: payload::stype::PAYLOAD_SEGMENT_ENTRY,
    //         base: cached_mem,
    //         data: &mut SectionReader::new(&Memory {}, mem + payload_offset, payload_size),
    //     },
    //     payload::Segment {
    //         typ: payload::stype::PAYLOAD_SEGMENT_ENTRY,
    //         base: cached_mem,
    //         data: &mut SectionReader::new(&Memory {}, mem + linuxboot_offset, linuxboot_size),
    //     },
    //     payload::Segment {
    //         typ: payload::stype::PAYLOAD_SEGMENT_ENTRY,
    //         base: cached_mem,
    //         data: &mut SectionReader::new(&Memory {}, mem + dtb_offset, dtb_size),
    //     },
    // ];
    // TODO: Get this from configuration
    // TODO: following boot stages
    1 // 1 => shutdown
}

extern "C" fn finish(_power_op: usize) -> ! {
    loop {
        unsafe { asm!("wfi") }
    }
}

/// This function is called on panic.
#[cfg_attr(not(test), panic_handler)]
fn panic(_info: &PanicInfo) -> ! {
    loop {} // todo
}
