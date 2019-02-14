extern crate cranelift;
extern crate cranelift_module;
extern crate cranelift_simplejit;
extern crate libc;
extern crate nix;
extern crate rustyline;
extern crate signal_hook;

use std::{mem, ptr, sync::mpsc, thread};

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
                tx1.send(EvalMsg::Signaled(signal)).unwrap();
            }
        }
    });

    // Create the JIT instance, which manages all generated functions and data.
    let mut jit = jit::JIT::new();

    let mut lineno = 0;
    let mut statement = Vec::<String>::new();
    let mut rl = Editor::<()>::new();
    loop {
        let readline = rl.readline(if statement.len() == 0 { ">>> " } else { "... " });
        match readline {
            Ok(line) => {
                if line.chars().all(char::is_whitespace) {
                    continue;
                }

                rl.add_history_entry(line.as_ref());

                let line = line + "\n";
                statement.push(line);

                let assembled_statement = statement.join("");
                match parser::statement(assembled_statement.as_ref()) {
                    Ok(_) => {
                        statement.clear();

                        let anon_fn = format!(
                            "fn __line{}() -> (x) {{\nx = {}}}\n",
                            lineno, assembled_statement
                        );
                        let compiled = match jit.compile(anon_fn.as_ref()) {
                            Ok(x) => x,
                            Err(e) => {
                                eprintln!("compile error: {}", e);
                                break;
                            }
                        };
                        let compiled = unsafe { mem::transmute::<_, fn() -> isize>(compiled) };

                        let tx2 = mpsc::Sender::clone(&tx);
                        let thread = thread::spawn(move || {
                            let thread_id = unsafe { libc::pthread_self() };
                            tx2.send(EvalMsg::IAmThread(thread_id)).unwrap();

                            unsafe {
                                signal_hook::register(signal_hook::SIGUSR1, || {
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
                    }
                    Err(ref e) if e.offset == assembled_statement.len() => {}
                    Err(_) => match parser::function(assembled_statement.as_ref()) {
                        Ok((name, params, the_return, stmts)) => {
                            statement.clear();
                            if let Err(e) = jit.compile_from_parsed(name, params, the_return, stmts)
                            {
                                eprintln!("compile error: {}", e);
                            }
                        }
                        Err(ref e) if e.offset == assembled_statement.len() => {}
                        Err(e) => {
                            statement.clear();
                            eprintln!(
                                "syntax error (offset {} len {})",
                                e.offset,
                                assembled_statement.len()
                            );
                        }
                    },
                }

                lineno += 1;
            }
            Err(ReadlineError::Interrupted) => {}
            Err(ReadlineError::Eof) => break,
            Err(e) => println!("rl err: {:?}", e),
        }
    }
}
