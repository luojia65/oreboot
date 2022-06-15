#![no_std]
#![no_main]
#![feature(default_alloc_error_handler)]
#![feature(naked_functions)]
#![feature(asm_sym, asm_const)]
#![feature(generator_trait)]

extern crate alloc;

mod execute;
mod feature;
mod hart_csr_utils;
mod peripheral;
mod runtime;

#[macro_use]
mod logging;

use core::arch::asm;
use core::panic::PanicInfo;
// use payloads::payload;
use buddy_system_allocator::LockedHeap;
use embedded_hal::digital::blocking::OutputPin;
use oreboot_soc::sunxi::d1::{
    ccu::Clocks,
    gpio::Gpio,
    pac::Peripherals,
    time::U32Ext,
    uart::{Config, Parity, Serial, StopBits, WordLength},
};

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
        "call   {heap_init}",
        "call   {main}",
        // Function `main` returns with hardware power operation type
        // which may be reboot or shutdown. Function `finish` would
        // perform these operations.
        "j      {finish}",
        stack      =   sym ENV_STACK,
        stack_size = const STACK_SIZE,
        heap_init  =   sym heap_init,
        main       =   sym main,
        finish     =   sym finish,
        options(noreturn)
    )
}

// stack which the bootloader environment would make use of.
#[link_section = ".bss.uninit"]
static mut ENV_STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];
const STACK_SIZE: usize = 1 * 1024; // 1KiB

extern "C" fn heap_init() {
    unsafe {
        SBI_HEAP
            .lock()
            .init(HEAP_SPACE.as_ptr() as usize, SBI_HEAP_SIZE)
    }
}

const SBI_HEAP_SIZE: usize = 8 * 1024; // 8KiB
static mut HEAP_SPACE: [u8; SBI_HEAP_SIZE] = [0; SBI_HEAP_SIZE];
#[global_allocator]
static SBI_HEAP: LockedHeap<32> = LockedHeap::empty();

static PLATFORM: &str = "T-HEAD Xuantie Platform";

// Function `main`. It would initialize an environment for the kernel.
// The environment does not exit when bootloading stage is finished;
// it remains in background to provide environment features which the
// kernel would make use of.
// Those features would include RISC-V SBI calls, instruction emulations,
// misaligned and so on.
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

    logging::set_logger(serial);

    // let w = &mut print::WriteTo::new(console);
    // writeln!(w, "## Loading payload\r").unwrap();

    // // see ../fixed-dtfs.dts
    // // TODO: adjust when DRAM driver is implemented / booting from SPI
    let mem = 0x4000_0000;
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

    init_pmp();
    // rustsbi::legacy_stdio::init_legacy_stdio_embedded_hal(serial);
    println!("oreboot: serial uart0 initialized");
    runtime::init();
    peripheral::init_peripheral();
    println!("RustSBI version {}\r", rustsbi::VERSION);
    println!("{}", rustsbi::LOGO);
    println!("Platform Name: {}\r", PLATFORM);
    println!(
        "Implementation: Oreboot version {}\r",
        env!("CARGO_PKG_VERSION")
    );
    unsafe {
        delegate_interrupt_exception();
    }
    hart_csr_utils::print_hart_csrs();
    hart_csr_utils::print_hart_pmp();
    println!("enter supervisor {}\r", mem);
    let (reset_type, reset_reason) =
        execute::execute_supervisor(mem, 0, 0 /* todo dtb offset */);
    println!("oreboot: reset reason = {}", reset_reason);
    reset_type
}

/**
 * from stock vendor OpenSBI:
 * PMP0    : 0x0000000040000000-0x000000004001ffff (A)
 * PMP1    : 0x0000000040000000-0x000000007fffffff (A,R,W,X)
 * PMP2    : 0x0000000000000000-0x0000000007ffffff (A,R,W)
 * PMP3    : 0x0000000009000000-0x000000000901ffff (
 */
// TODO: protect oreboot; this is an all-accessible config
fn init_pmp() {
    use riscv::register::*;
    let cfg = 0x0f0f0f0f0fusize;
    pmpcfg0::write(cfg);
    // pmpcfg2::write(0);
    pmpaddr0::write(0x40000000usize >> 2);
    pmpaddr1::write(0x40200000usize >> 2);
    pmpaddr2::write(0x80000000usize >> 2);
    pmpaddr3::write(0xc0000000usize >> 2);
    pmpaddr4::write(0xffffffffusize >> 2);
}

unsafe fn delegate_interrupt_exception() {
    use riscv::register::{medeleg, mideleg, mie};
    mideleg::set_sext();
    mideleg::set_stimer();
    mideleg::set_ssoft();
    // p 35, table 3.6
    medeleg::set_instruction_misaligned();
    medeleg::set_instruction_fault();
    // Do not medeleg::set_illegal_instruction();
    // We need to handle sfence.VMA and timer access in SBI.
    medeleg::set_breakpoint();
    medeleg::set_load_misaligned();
    medeleg::set_load_fault(); // PMP violation, shouldn't be hit
    medeleg::set_store_misaligned();
    medeleg::set_store_fault();
    medeleg::set_user_env_call();
    // Do not delegate env call from S-mode nor M-mode
    medeleg::set_instruction_page_fault();
    medeleg::set_load_page_fault();
    medeleg::set_store_page_fault();
    mie::set_msoft();
}

extern "C" fn finish(reset_type: usize) -> ! {
    use rustsbi::reset::*;
    match reset_type {
        RESET_TYPE_SHUTDOWN => loop {
            unsafe { asm!("wfi") }
        },
        RESET_TYPE_COLD_REBOOT => todo!(),
        RESET_TYPE_WARM_REBOOT => todo!(),
        _ => unimplemented!(),
    }
}

/// This function is called on panic.
#[cfg_attr(not(test), panic_handler)]
fn panic(_info: &PanicInfo) -> ! {
    loop {} // todo
}
