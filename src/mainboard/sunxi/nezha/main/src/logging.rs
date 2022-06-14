// TODO: rearrange and move to src/console

use embedded_hal::serial::nb::Write;
use nb::block;
use oreboot_soc::sunxi::d1::{
    gpio::{
        portb::{PB8, PB9},
        Function,
    },
    pac::UART0,
    uart::{self, Serial},
};
use spin::{Mutex, Once};

#[doc(hidden)]
pub(crate) static LOGGER: Once<LockedLogger> = Once::new();

type S = Wrap<Serial<UART0, (PB8<Function<6>>, PB9<Function<6>>)>>;

// type `Serial` is declared out of this crate, avoid orphan rule
pub(crate) struct Wrap<T>(T);

#[doc(hidden)]
pub(crate) struct LockedLogger {
    pub(crate) inner: Mutex<S>,
}

impl ufmt::uWrite for S {
    type Error = uart::Error;
    #[inline]
    fn write_str(&mut self, s: &str) -> Result<(), uart::Error> {
        for byte in s.as_bytes() {
            block!(self.0.write(*byte))?
        }
        block!(self.0.flush())?;
        Ok(())
    }
}

#[inline]
pub fn set_logger(serial: Serial<UART0, (PB8<Function<6>>, PB9<Function<6>>)>) {
    LOGGER.call_once(|| LockedLogger {
        inner: Mutex::new(Wrap(serial)),
    });
}

#[macro_export(local_inner_macros)]
macro_rules! print {
    ($($arg:tt)*) => ({
        let mut logger = $crate::logging::LOGGER.wait().inner.lock();
        let ans = ufmt::uwrite!(logger, $($arg)*);
        drop(logger);
        ans
    });
}

#[macro_export(local_inner_macros)]
macro_rules! println {
    () => ($crate::print!("\r\n"));
    ($fmt: literal $(, $($arg: tt)+)?) => ({
        let mut logger = $crate::logging::LOGGER.wait().inner.lock();
        let ans = ufmt::uwrite!(logger, $fmt $(, $($arg)+)?);
        drop(logger);
        let _ = $crate::print!("\r\n");
        ans
    });
}
