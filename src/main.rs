extern crate cranelift;
extern crate cranelift_module;
extern crate cranelift_simplejit;

use std::{io, io::Read, mem, process};

mod frontend;
mod jit;

fn main() {
    // Create the JIT instance, which manages all generated functions and data.
    let mut jit = jit::JIT::new();

    let mut code = String::new();
    {
        let stdin = io::stdin();
        let mut handle = stdin.lock();
        handle.read_to_string(&mut code);
    }

    let func = jit.compile(&code).unwrap_or_else(|msg| {
        eprintln!("error: {}", msg);
        process::exit(1);
    });

    let func = unsafe { mem::transmute::<_, fn() -> isize>(func) };

    let ret = func();
    process::exit(ret as i32);
}
