#![cfg(target_os = "linux")]

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

fn network_sockets(pid: u32) -> Vec<String> {
    let fds = std::fs::read_dir(format!("/proc/{pid}/fd")).unwrap();
    let links = fds.map(|fd| std::fs::read_link(fd.unwrap().path()).unwrap_or_default());
    let inodes: Vec<String> = links
        .filter_map(|l| {
            Some(
                l.to_str()?
                    .strip_prefix("socket:[")?
                    .strip_suffix(']')?
                    .into(),
            )
        })
        .collect();
    let tables = ["tcp", "tcp6", "udp", "udp6", "raw", "raw6"];
    let rows = tables.map(|t| std::fs::read_to_string(format!("/proc/{pid}/net/{t}")).unwrap());
    let rows = rows.iter().flat_map(|r| r.lines().skip(1));
    let ours = |row: &&str| {
        inodes
            .iter()
            .any(|i| row.split_whitespace().nth(9) == Some(i))
    };
    rows.filter(ours).map(str::to_owned).collect()
}

#[test]
fn bramley_mcp_speaks_stdio_discloses_and_opens_no_network_socket() {
    let target =
        std::fs::canonicalize(concat!(env!("CARGO_MANIFEST_DIR"), "/../../target")).unwrap();
    let dir = target.join(format!("mcp{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_bramley"))
        .arg("mcp")
        .env("XDG_RUNTIME_DIR", &dir)
        .env("BRAMLEY_CONFIG_DIR", dir.join("config"))
        .env("TMPDIR", &dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut replies = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut ask = |message: Value, answered: bool| {
        writeln!(stdin, "{message}").unwrap();
        let reply = answered.then(|| replies.next().unwrap().unwrap());
        reply.map_or(Value::Null, |r| serde_json::from_str(&r).unwrap())
    };
    let hello = json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}});
    let init = ask(
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":hello}),
        true,
    );
    ask(
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        false,
    );
    let listed = ask(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}), true);
    let play = json!({"name":"play","arguments":{"id":"6781024437"}});
    let played = ask(
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":play}),
        true,
    );
    let sockets = network_sockets(child.id());
    drop(ask);
    drop(stdin);
    let status = child.wait().unwrap();
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    let disclosed = dir.join("config/mcp-disclosed").exists();
    std::fs::remove_dir_all(&dir).unwrap();

    let init = &init["result"];
    assert_eq!(init["protocolVersion"], "2025-06-18");
    assert!(init["capabilities"]["tools"].is_object() && init["serverInfo"]["name"] == "bramley");
    assert!(
        init["instructions"]
            .as_str()
            .unwrap()
            .contains("goes to the agent you connected")
    );
    let tools = listed["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    let expected =
        "search play pause resume next previous seek queue_add now_playing library_playlists";
    assert_eq!(names.join(" "), expected);
    assert_eq!(tools[0]["inputSchema"]["required"], json!(["term"]));
    assert_eq!(
        tools[6]["inputSchema"]["properties"]["seconds"]["type"],
        "number"
    );
    assert_eq!(played["result"]["isError"], true);
    let why = played["result"]["content"][0]["text"].as_str().unwrap();
    assert!(why.starts_with("Bramley is not running"), "{why}");
    assert!(
        stderr.contains("goes to the agent you connected") && disclosed,
        "{stderr}"
    );
    assert_eq!(sockets, Vec::<String>::new());
    assert!(status.success(), "{status:?} {stderr}");
}
