//! Implements `Syscalls` for all types that implement `RawSyscalls`.

use crate::{
    exit_id, exit_on_drop, return_variant, subscribe, syscall_class, yield_id, CommandReturn,
    ErrorCode, RawSyscalls, Register, ReturnVariant, Subscribe, Syscalls, Upcall,
    YieldNoWaitReturn,
};

impl<S: RawSyscalls> Syscalls for S {
    // -------------------------------------------------------------------------
    // Yield
    // -------------------------------------------------------------------------

    fn yield_no_wait() -> YieldNoWaitReturn {
        let mut flag = core::mem::MaybeUninit::<YieldNoWaitReturn>::uninit();

        unsafe {
            // Flag can be uninitialized here because the kernel promises to
            // only write to it, not read from it. MaybeUninit guarantees that
            // it is safe to write a YieldNoWaitReturn into it.
            Self::yield2([yield_id::NO_WAIT.into(), flag.as_mut_ptr().into()]);

            // yield-no-wait guarantees it sets (initializes) flag before
            // returning.
            flag.assume_init()
        }
    }

    fn yield_wait() {
        // Safety: yield-wait does not return a value, which satisfies yield1's
        // requirement. The yield-wait system call cannot trigger undefined
        // behavior on its own in any other way.
        unsafe {
            Self::yield1([yield_id::WAIT.into()]);
        }
    }

    // -------------------------------------------------------------------------
    // Subscribe
    // -------------------------------------------------------------------------

    fn subscribe<
        'scope,
        IDS: subscribe::SupportsId<DRIVER_NUM, SUBSCRIBE_NUM>,
        U: Upcall<IDS>,
        CONFIG: subscribe::Config,
        const DRIVER_NUM: u32,
        const SUBSCRIBE_NUM: u32,
    >(
        _subscribe: &Subscribe<'scope, Self, DRIVER_NUM, SUBSCRIBE_NUM>,
        upcall: &'scope U,
    ) -> Result<(), ErrorCode> {
        // The upcall function passed to the Tock kernel.
        //
        // Safety: data must be a reference to a valid instance of U.
        unsafe extern "C" fn kernel_upcall<S: Syscalls, IDS, U: Upcall<IDS>>(
            arg0: u32,
            arg1: u32,
            arg2: u32,
            data: Register,
        ) {
            let exit: exit_on_drop::ExitOnDrop<S> = Default::default();
            let upcall: *const U = data.into();
            unsafe { &*upcall }.upcall(arg0, arg1, arg2);
            core::mem::forget(exit);
        }

        // Inner function that does the majority of the work. This is not
        // monomorphized over DRIVER_NUM and SUBSCRIBE_NUM to keep code size
        // small.
        //
        // Safety: upcall_fcn must be kernel_upcall<S, IDS, U> and upcall_data
        // must be a reference to an instance of U that will remain valid as
        // long as the 'scope lifetime is alive. Can only be called if a
        // Subscribe<'scope, S, driver_num, subscribe_num> exists.
        unsafe fn inner<S: Syscalls, CONFIG: subscribe::Config>(
            driver_num: u32,
            subscribe_num: u32,
            upcall_fcn: Register,
            upcall_data: Register,
        ) -> Result<(), ErrorCode> {
            // Safety: syscall4's documentation indicates it can be used to call
            // Subscribe. These arguments follow TRD104. kernel_upcall has the
            // required signature. This function's preconditions mean that
            // upcall is a reference to an instance of U that will remain valid
            // until the 'scope lifetime is alive The existence of the
            // Subscribe<'scope, Self, DRIVER_NUM, SUBSCRIBE_NUM> guarantees
            // that if this Subscribe succeeds then the upcall will be cleaned
            // up before the 'scope lifetime ends, guaranteeing that upcall is
            // still alive when kernel_upcall is invoked.
            let [r0, r1, _, _] = unsafe {
                S::syscall4::<{ syscall_class::SUBSCRIBE }>([
                    driver_num.into(),
                    subscribe_num.into(),
                    upcall_fcn,
                    upcall_data,
                ])
            };

            let return_variant: ReturnVariant = r0.as_u32().into();
            // TRD 104 guarantees that Subscribe returns either Success with 2
            // U32 or Failure with 2 U32. We check the return variant by
            // comparing against Failure with 2 U32 for 2 reasons:
            //
            //   1. On RISC-V with compressed instructions, it generates smaller
            //      code. FAILURE_2_U32 has value 2, which can be loaded into a
            //      register with a single compressed instruction, whereas
            //      loading SUCCESS_2_U32 uses an uncompressed instruction.
            //   2. In the event the kernel malfuctions and returns a different
            //      return variant, the success path is actually safer than the
            //      failure path. The failure path assumes that r1 contains an
            //      ErrorCode, and produces UB if it has an out of range value.
            //      Incorrectly assuming the call succeeded will not generate
            //      unsoundness, and will likely lead to the application
            //      hanging.
            if return_variant == return_variant::FAILURE_2_U32 {
                // Safety: TRD 104 guarantees that if r0 is Failure with 2 U32,
                // then r1 will contain a valid error code. ErrorCode is
                // designed to be safely transmuted directly from a kernel error
                // code.
                return Err(unsafe { core::mem::transmute(r1.as_u32() as u16) });
            }

            // r0 indicates Success with 2 u32s. Confirm the null upcall was
            // returned, and it if wasn't then call the configured function.
            // We're relying on the optimizer to remove this branch if
            // returned_nonnull_upcall is a no-op.
            // Note: TRD 104 specifies that the null upcall has address 0,
            // not necessarily a null pointer.
            let returned_upcall: usize = r1.into();
            if returned_upcall != 0usize {
                CONFIG::returned_nonnull_upcall(driver_num, subscribe_num);
            }
            Ok(())
        }

        let upcall_fcn = (kernel_upcall::<S, IDS, U> as usize).into();
        let upcall_data = (upcall as *const U).into();
        // Safety: upcall's type guarantees it is a reference to a U that will
        // remain valid for at least the 'scope lifetime. _subscribe is a
        // reference to a Subscribe<'scope, Self, DRIVER_NUM, SUBSCRIBE_NUM>,
        // proving one exists. upcall_fcn and upcall_data are derived in ways
        // that satisfy inner's requirements.
        unsafe { inner::<Self, CONFIG>(DRIVER_NUM, SUBSCRIBE_NUM, upcall_fcn, upcall_data) }
    }

    fn unsubscribe(driver_num: u32, subscribe_num: u32) {
        unsafe {
            // syscall4's documentation indicates it can be used to call
            // Subscribe. The upcall pointer passed is the null upcall, which
            // cannot cause undefined behavior on its own.
            Self::syscall4::<{ syscall_class::SUBSCRIBE }>([
                driver_num.into(),
                subscribe_num.into(),
                0usize.into(),
                0usize.into(),
            ]);
        }
    }

    // -------------------------------------------------------------------------
    // Command
    // -------------------------------------------------------------------------

    fn command(driver_id: u32, command_id: u32, argument0: u32, argument1: u32) -> CommandReturn {
        unsafe {
            // syscall4's documentation indicates it can be used to call
            // Command. The Command system call cannot trigger undefined
            // behavior on its own.
            let [r0, r1, r2, r3] = Self::syscall4::<{ syscall_class::COMMAND }>([
                driver_id.into(),
                command_id.into(),
                argument0.into(),
                argument1.into(),
            ]);

            // Because r0 and r1 are returned directly from the kernel, we are
            // guaranteed that if r0 represents a failure variant then r1 is an
            // error code.
            CommandReturn::new(r0.as_u32().into(), r1.as_u32(), r2.as_u32(), r3.as_u32())
        }
    }

    // -------------------------------------------------------------------------
    // Exit
    // -------------------------------------------------------------------------

    fn exit_terminate(exit_code: u32) -> ! {
        unsafe {
            // syscall2's documentation indicates it can be used to call Exit.
            // The exit system call cannot trigger undefined behavior on its
            // own.
            Self::syscall2::<{ syscall_class::EXIT }>([
                exit_id::TERMINATE.into(),
                exit_code.into(),
            ]);
            // TRD104 indicates that exit-terminate MUST always succeed and so
            // never return.
            core::hint::unreachable_unchecked()
        }
    }

    fn exit_restart(exit_code: u32) -> ! {
        unsafe {
            // syscall2's documentation indicates it can be used to call Exit.
            // The exit system call cannot trigger undefined behavior on its
            // own.
            Self::syscall2::<{ syscall_class::EXIT }>([exit_id::RESTART.into(), exit_code.into()]);
            // TRD104 indicates that exit-restart MUST always succeed and so
            // never return.
            core::hint::unreachable_unchecked()
        }
    }
}
