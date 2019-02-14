extern crate cranelift;
extern crate cranelift_module;
extern crate cranelift_simplejit;
extern crate libc;
extern crate nix;
extern crate rustyline;
extern crate signal_hook;

use std::{mem, panic, ptr, sync::mpsc, thread};

use rustyline::{error::ReadlineError, Editor};
use signal_hook::iterator::Signals;

mod frontend;
mod jit;

use frontend::parser;

#[derive(Debug)]
enum EvalMsg {
    IAmThread(libc::pthread_t),
    Done(isize),
    Signaled(libc::c_int),
}

fn main() {
    let (tx, rx) = mpsc::channel();
    let tx1 = mpsc::Sender::clone(&tx);
    thread::spawn(move || {
        let signals = Signals::new(&[signal_hook::SIGINT])
            .expect("ctrl-c handler failed, you won't be able to terminate loops");
        loop {
            for signal in signals.pending() {
                tx1.send(EvalMsg::Signaled(signal));
            }
        }
    });

    // Create the JIT instance, which manages all generated functions and data.
    let mut jit = jit::JIT::new();

    let mut lineno = 0;
    let mut new_function = Vec::<String>::new();
    let mut rl = Editor::<()>::new();
    loop {
        let readline = rl.readline(if new_function.len() == 0 {
            ">>> "
        } else {
            "... "
        });
        match readline {
            Ok(line) => {
                if line.chars().all(char::is_whitespace) {
                    continue;
                }

                rl.add_history_entry(line.as_ref());

                let line = line + "\n";

                if new_function.len() != 0 {
                    new_function.push(line);
                    let assembled_fn = new_function.join("");

                    if let Ok((name, params, the_return, stmts)) =
                        parser::function(assembled_fn.as_ref())
                    {
                        new_function.clear();

                        if let Err(e) = jit.compile_from_parsed(name, params, the_return, stmts) {
                            eprintln!("compile error: {}", e);
                        }
                    }
                } else if let Ok(_) = parser::function_definition(line.as_ref()) {
                    new_function.push(line);
                } else if let Ok(_) = parser::statement(line.as_ref()) {
                    let anon_fn = format!("fn __line{}() -> (x) {{\nx = {}}}\n", lineno, line);

                    let compiled = match jit.compile(anon_fn.as_ref()) {
                        Ok(x) => x,
                        Err(e) => {
                            eprintln!("compile error: {}", e);
                            continue;
                        }
                    };

                    let compiled = unsafe { mem::transmute::<_, fn() -> isize>(compiled) };

                    let tx2 = mpsc::Sender::clone(&tx);
                    let thread = thread::spawn(move || {
                        let thread_id = unsafe { libc::pthread_self() };
                        tx2.send(EvalMsg::IAmThread(thread_id)).unwrap();

                        unsafe {
                            signal_hook::register(signal_hook::SIGUSR1, || unsafe {
                                libc::pthread_exit(ptr::null_mut::<libc::c_void>());
                            })
                            .unwrap();
                        }

                        let ret = compiled();
                        tx2.send(EvalMsg::Done(ret)).unwrap();
                    });

                    let mut thread_id = None;
                    for msg in rx.iter() {
                        match msg {
                            EvalMsg::IAmThread(id) => thread_id = Some(id),
                            EvalMsg::Done(ret) => {
                                println!("{}", ret);
                                thread.join().unwrap();
                                break;
                            }
                            EvalMsg::Signaled(_) => {
                                if let Some(id) = thread_id {
                                    unsafe {
                                        libc::pthread_kill(id, signal_hook::SIGUSR1 as i32);
                                    }
                                }
                                break;
                            }
                        }
                    }
                } else {
                    eprintln!("syntax error");
                }

                lineno += 1;
            }
            Err(ReadlineError::Interrupted) => {}
            Err(ReadlineError::Eof) => break,
            Err(e) => println!("rl err: {:?}", e),
        }
    }
}
