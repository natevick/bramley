use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use deno_core::{JsRuntime, RuntimeOptions, v8};

const PRELUDE: &str = include_str!("../../shared/prelude.js");
const APP: &str = include_str!("../../shared/app.js");

struct Args {
    musickit: String,
    hold_after_point: u64,
    max_wait: u64,
    truncate: Option<f64>,
    mode: String,
}

fn parse_args() -> Args {
    let mut a = Args { musickit: String::new(), hold_after_point: 34, max_wait: 30, truncate: None, mode: "logged".into() };
    let v: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < v.len() {
        match v[i].as_str() {
            "--musickit" => { a.musickit = v[i + 1].clone(); i += 1; }
            "--hold" => { a.hold_after_point = v[i + 1].parse().unwrap(); i += 1; }
            "--max-wait" => { a.max_wait = v[i + 1].parse().unwrap(); i += 1; }
            "--truncate" => { a.truncate = Some(v[i + 1].parse().unwrap()); i += 1; }
            "--mode" => { a.mode = v[i + 1].clone(); i += 1; }
            other => panic!("unknown arg {other}"),
        }
        i += 1;
    }
    a
}

fn eval_bool(rt: &mut JsRuntime, code: &'static str) -> bool {
    match rt.execute_script("poll.js", code) {
        Ok(g) => {
            deno_core::scope!(scope, rt);
            let v = v8::Local::new(scope, g);
            v.is_true()
        }
        Err(_) => false,
    }
}

fn eval_string(rt: &mut JsRuntime, code: &'static str) -> Result<String, String> {
    match rt.execute_script("report.js", code) {
        Ok(g) => {
            deno_core::scope!(scope, rt);
            let v = v8::Local::new(scope, g);
            Ok(v.to_rust_string_lossy(scope))
        }
        Err(e) => Err(format!("{e}")),
    }
}

fn main() {
    let t_start = Instant::now();
    let epoch_ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as f64;
    let args = parse_args();
    let mut src = String::from_utf8(std::fs::read(&args.musickit).expect("read musickit")).expect("utf8");
    let full_len = src.len();
    if let Some(f) = args.truncate {
        let mut n = (src.len() as f64 * f) as usize;
        while !src.is_char_boundary(n) { n -= 1; }
        src.truncate(n);
    }
    println!("PID {}", std::process::id());
    println!("SRC {{\"fullBytes\":{},\"evalBytes\":{}}}", full_len, src.len());

    let tokio_rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let _guard = tokio_rt.enter();
    let mut rt = JsRuntime::new(RuntimeOptions::default());
    let t_engine = t_start.elapsed();

    let mode = args.mode.clone();
    let boot = format!(
        "globalThis.__host_print = (s) => Deno.core.print(String(s) + \"\\n\", false); globalThis.__host_t0 = {epoch_ms}; globalThis.__spike_mode = {mode:?};"
    );
    rt.execute_script("boot.js", boot).expect("boot");
    let a = Instant::now();
    rt.execute_script("prelude.js", PRELUDE).expect("prelude");
    let t_prelude = a.elapsed();
    let b = Instant::now();
    let mk_error = rt.execute_script("musickit.js", src).err().map(|e| format!("{e}"));
    let t_mk = b.elapsed();
    if let Err(e) = rt.execute_script("app.js", APP) {
        println!("APPERR {e}");
    }
    rt.v8_isolate().perform_microtask_checkpoint();
    let mk_defined_ms = t_start.elapsed().as_secs_f64() * 1000.0;
    println!(
        "TIMING {{\"engineInitMs\":{:.2},\"preludeMs\":{:.2},\"mkEvalMs\":{:.2},\"mkDefinedSinceStartMs\":{:.2},\"mkEvalError\":{}}}",
        t_engine.as_secs_f64() * 1000.0,
        t_prelude.as_secs_f64() * 1000.0,
        t_mk.as_secs_f64() * 1000.0,
        mk_defined_ms,
        match &mk_error { Some(e) => format!("{:?}", e), None => "null".into() }
    );

    let wait_deadline = Instant::now() + Duration::from_secs(args.max_wait);
    let mut point_at: Option<Instant> = None;
    loop {
        rt.v8_isolate().perform_microtask_checkpoint();
        let _ = rt.execute_script("tick.js", "__host_tick()");
        rt.v8_isolate().perform_microtask_checkpoint();
        let reached = eval_bool(&mut rt, "__spike.point !== null");
        if reached && point_at.is_none() { point_at = Some(Instant::now()); }
        match point_at {
            Some(p) if p.elapsed() >= Duration::from_secs(args.hold_after_point) => break,
            None if Instant::now() >= wait_deadline => { println!("NOPOINT"); break; }
            _ => {}
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    match eval_string(&mut rt, "__spike.report()") {
        Ok(s) => println!("REPORT {s}"),
        Err(e) => println!("REPORTERR {e}"),
    }
}
