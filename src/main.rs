extern crate cranelift;
extern crate cranelift_module;
extern crate cranelift_simplejit;
extern crate rustyline;

use std::{io, io::Read, mem, process};

use rustyline::{error::ReadlineError, Editor};

mod frontend;
mod jit;

use frontend::parser;

fn main() {
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

                    println!("{}", compiled());
                } else {
                    eprintln!("syntax error");
                }

                lineno += 1;

                //match parser::function_definition(line.as_ref()) {
                //    Ok((name, params, the_return, stmts)) => { jit
                //        .compile_from_parsed(name, params, the_return, stmts)
                //        .unwrap_or_else(|msg| {
                //            eprintln!("error: {}", msg);
                //            process::exit(1);
                //        }); },
                //    Err(parser::ParseError { expected, .. }) => { println!("expected: {:?}", expected) },
                //    Err(e) => {}
                //}

                //} else if let Ok(parsed) = parser::statement(line.as_ref()) {
                //    if lines_buf
                //}
                //match parser::function(line.as_ref()) {
                //    Ok(parsed) => {}
                //    Err(e) => println!("fn err: {:?}", e),
                //}
                //if let Ok(stmt) = parser::statement(line.as_ref()) {
                //    if lines_buf.len() == 0 {
                //        println!("running");
                //        // just run
                //    } else {
                //        lines_buf.push(line);
                //        println!("{}", lines_buf.join("\n"));
                //    }
                //}
            }
            Err(ReadlineError::Interrupted) => {}
            Err(ReadlineError::Eof) => break,
            Err(e) => println!("rl err: {:?}", e),
        }
    }

    process::exit(0);

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
