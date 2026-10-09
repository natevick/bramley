use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rquickjs::{CatchResultExt, Context, Function, Runtime, context::EvalOptions};

const PRELUDE: &str = include_str!("../../shared/prelude.js");
const APP: &str = include_str!("../../shared/app.js");

fn opts(name: &str) -> EvalOptions {
    let mut o = EvalOptions::default();
    o.strict = false;
    o.global = true;
    o.filename = Some(name.into());
    o
}

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

    let rt = Runtime::new().expect("runtime");
    rt.set_max_stack_size(4 * 1024 * 1024);
    let ctx = Context::full(&rt).expect("context");
    let t_engine = t_start.elapsed();

    let mut t_prelude = Duration::ZERO;
    let mut t_mk = Duration::ZERO;
    let mut mk_error: Option<String> = None;
    ctx.with(|ctx| {
        let g = ctx.globals();
        g.set("__host_print", Function::new(ctx.clone(), |s: String| { println!("{s}"); }).unwrap()).unwrap();
        g.set("__host_t0", epoch_ms).unwrap();
        g.set("__spike_mode", args.mode.clone()).unwrap();
        let a = Instant::now();
        if let Err(e) = ctx.eval_with_options::<(), _>(PRELUDE, opts("prelude.js")).catch(&ctx) {
            panic!("prelude failed: {e}");
        }
        t_prelude = a.elapsed();
        let b = Instant::now();
        if let Err(e) = ctx.eval_with_options::<(), _>(src.as_bytes().to_vec(), opts("musickit.js")).catch(&ctx) {
            mk_error = Some(format!("{e}"));
        }
        t_mk = b.elapsed();
        if let Err(e) = ctx.eval_with_options::<(), _>(APP, opts("app.js")).catch(&ctx) {
            println!("APPERR {e}");
        }
    });
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
        loop {
            match rt.execute_pending_job() {
                Ok(true) => continue,
                Ok(false) => break,
                Err(e) => { println!("JOBERR {e:?}"); }
            }
        }
        let reached = ctx.with(|ctx| {
            let _ = ctx.eval::<i32, _>("__host_tick()").catch(&ctx);
            ctx.eval::<bool, _>("__spike.point !== null").unwrap_or(false)
        });
        if reached && point_at.is_none() { point_at = Some(Instant::now()); }
        match point_at {
            Some(p) if p.elapsed() >= Duration::from_secs(args.hold_after_point) => break,
            None if Instant::now() >= wait_deadline => { println!("NOPOINT"); break; }
            _ => {}
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    ctx.with(|ctx| {
        match ctx.eval::<String, _>("__spike.report()").catch(&ctx) {
            Ok(s) => println!("REPORT {s}"),
            Err(e) => println!("REPORTERR {e}"),
        }
    });
}
