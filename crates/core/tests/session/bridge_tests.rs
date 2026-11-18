//! The control port's protocol (`bridge`).

use crate::bridge::*;
use serde_json::{Value, json};

use crate::engine::{Session, find};

const T: &str = "0123-token";

fn walls(s: &Session) -> usize {
    s.model.elements.values().filter(|e| matches!(e, crate::model::Element::Wall(_))).count()
}

fn line(v: Value) -> String {
    v.to_string()
}

fn execute(command: &str, params: Value) -> Request {
    Request::Execute { command: command.into(), params, read_only: false }
}

/// A wall along y.
fn wall(y: i32) -> Request {
    execute("wall", json!({"points": [[0, y], [4000, y]]}))
}

#[test]
fn a_request_is_read_with_its_token_and_what_is_wrong_with_one_is_said() {
    assert_eq!(parse(&line(json!({"token": T, "op": "hello"})), T), Ok(Request::Hello));
    assert_eq!(
        parse(&line(json!({"token": T, "op": "execute", "command": "wall", "params": {"points": [[0, 0], [1000, 0]]}})), T),
        Ok(execute("wall", json!({"points": [[0, 0], [1000, 0]]})))
    );
    assert_eq!(
        parse(&line(json!({"token": T, "op": "execute", "command": "undo"})), T),
        Ok(execute("undo", json!({}))),
        "no params is none"
    );
    assert_eq!(
        parse(&line(json!({"token": T, "op": "script", "text": "inspect", "atomic": true})), T),
        Ok(Request::Script { text: "inspect".into(), atomic: true, dry_run: false, read_only: false })
    );
    // The token first: a request without it, or with another, says nothing else.
    for request in [
        json!({"op": "hello"}),
        json!({"token": "x", "op": "hello"}),
        json!({"token": 5, "op": "hello"}),
        json!({"token": "", "op": "bogus"}),
    ] {
        assert_eq!(parse(&line(request.clone()), T), Err(Error::Token), "{request}");
    }
    let bad = |request: Value| match parse(&line(request.clone()), T) {
        Err(Error::Request(why)) => why,
        other => panic!("{request}: {other:?}"),
    };
    assert!(bad(json!({"token": T})).contains("`op`"));
    assert!(bad(json!({"token": T, "op": "format"})).contains("unknown op `format`"));
    assert!(bad(json!({"token": T, "op": "execute"})).contains("`command`"));
    assert!(bad(json!({"token": T, "op": "execute", "command": 7})).contains("`command`"));
    assert!(bad(json!({"token": T, "op": "execute", "command": "wall", "params": [1]})).contains("`params`"));
    assert!(bad(json!({"token": T, "op": "script"})).contains("`text`"));
    assert!(bad(json!({"token": T, "op": "script", "text": "x", "atomic": "yes"})).contains("`atomic`"));
    assert!(matches!(parse("[1, 2]", T), Err(Error::Request(why)) if why.contains("object")));
    assert!(matches!(parse("{not json", T), Err(Error::Request(why)) if why.starts_with("not JSON")));
    assert!(matches!(parse("", T), Err(Error::Request(_))));
}

#[test]
fn tokens_are_the_same_only_when_every_byte_is() {
    assert!(same("abc", "abc") && same("", ""));
    assert!(!same("abc", "abd") && !same("abc", "ab") && !same("ab", "abc") && !same("abc", "ABC"));
}

#[test]
fn a_request_runs_on_the_session_as_a_command_does_and_the_reply_says_how_it_went() {
    let mut s = Session::new();
    let ok: Value = serde_json::from_str(&reply(run(&mut s, &wall(0), Access::Edit))).unwrap();
    assert!(ok["ok"]["id"].is_u64() || ok["ok"].is_object(), "{ok}");
    assert_eq!(walls(&s), 1, "the wall is in the session");
    // A command that fails is an error reply, and the model stays.
    let door = execute("door", json!({"at": [99000, 99000]}));
    let err: Value = serde_json::from_str(&reply(run(&mut s, &door, Access::Edit))).unwrap();
    assert!(err["error"].as_str().is_some_and(|e| e.contains("no wall")), "{err}");
    assert_eq!(walls(&s), 1);
    // hello says what is there.
    let hello = run(&mut s, &Request::Hello, Access::Edit).unwrap();
    assert_eq!((hello["app"].as_str(), hello["project"].as_str()), (Some("bimcraft"), Some(s.model.name.as_str())));
    // catalog is the engine's list, and the window's plugin commands with it.
    let catalog = run(&mut s, &Request::Catalog, Access::Edit).unwrap();
    assert!(catalog.as_array().is_some_and(|list| list.iter().any(|c| c["id"] == json!("wall"))));
    // undo is a step like the others.
    run(&mut s, &execute("undo", json!({})), Access::Edit).unwrap();
    assert_eq!(walls(&s), 0);
}

#[test]
fn what_would_replace_the_project_is_refused_by_any_name_and_a_script_is_read_before_it_runs() {
    let mut s = Session::new();
    run(&mut s, &wall(0), Access::Edit).unwrap();
    let kept = std::sync::Arc::clone(&s.model);
    for name in REFUSED {
        assert!(find(name).is_some_and(|c| c.id == *name), "`{name}` is a command id");
        for spelled in [name.to_string(), name.to_uppercase()] {
            let e = run(&mut s, &execute(&spelled, json!({})), Access::Edit).unwrap_err();
            assert!(e.contains("would replace the project"), "{name}: {e}");
        }
    }
    // By a shortcut: whatever an alias of a refused command is.
    for c in REFUSED.iter().filter_map(|n| find(n)) {
        for alias in c.aliases {
            let e = run(&mut s, &execute(alias, json!({})), Access::Edit).unwrap_err();
            assert!(e.contains("would replace the project"), "{alias}: {e}");
        }
    }
    // A script with one such line runs none of its lines, and says which.
    let text = "wall {\"points\": [[0, 5000], [4000, 5000]]}\n# a comment\nnew {}\n";
    for atomic in [false, true] {
        let e = run(&mut s, &Request::Script { text: text.into(), atomic, dry_run: false, read_only: false }, Access::Edit).unwrap_err();
        assert!(e.starts_with("line 3: ") && e.contains("`new`"), "{e}");
    }
    assert!(std::sync::Arc::ptr_eq(&kept, &s.model), "nothing ran");
    // The rest of a script runs, and the reply is each line's.
    let ok = run(
        &mut s,
        &Request::Script {
            text: "wall {\"points\": [[0, 5000], [4000, 5000]]}\ninspect".into(),
            atomic: true,
            dry_run: false,
            read_only: false,
        },
        Access::Edit,
    )
    .unwrap();
    assert_eq!(ok.as_array().map(Vec::len), Some(2));
    assert_eq!(walls(&s), 2);
}

#[test]
fn a_window_that_lets_agents_only_read_refuses_their_edits_says_why_and_still_answers_what_reads() {
    let mut s = Session::new();
    run(&mut s, &wall(0), Access::Edit).unwrap();
    let kept = std::sync::Arc::clone(&s.model);
    // The hello says so, and an edit, an undo, a script holding one, by any name, are refused and name the Settings box.
    assert_eq!(run(&mut s, &Request::Hello, Access::Read).unwrap()["read_only"], true);
    assert_eq!(run(&mut s, &Request::Hello, Access::Edit).unwrap()["read_only"], false);
    let script = |text: &str, atomic| Request::Script { text: text.into(), atomic, dry_run: false, read_only: false };
    for request in [
        wall(5000),
        execute("WA", json!({"points": [[0, 5000], [4000, 5000]]})),
        execute("undo", json!({})),
        script("inspect\nwall {\"points\": [[0, 5000], [4000, 5000]]}", false),
        script("inspect\nwall {\"points\": [[0, 5000], [4000, 5000]]}", true),
    ] {
        let e = run(&mut s, &request, Access::Read).unwrap_err();
        assert!(e.contains("read-only for agents") && e.contains("File > Settings > Agents"), "{request:?}: {e}");
    }
    assert!(std::sync::Arc::ptr_eq(&kept, &s.model), "nothing changed");
    assert_eq!(s.history(), ["Wall"], "nor the history");
    assert_eq!(s.read_only, None, "the window's own commands stay the user's");
    // What reads answers.
    assert_eq!(run(&mut s, &execute("inspect", json!({})), Access::Read).unwrap()["counts"]["wall"], 1);
    assert!(run(&mut s, &script("inspect\nschedule {\"category\": \"walls\"}", true), Access::Read).is_ok());
    // Replacing the project is refused whatever the window lets, and says what the user does instead.
    for access in [Access::Edit, Access::Read] {
        let e = run(&mut s, &execute("open", json!({"path": "/nowhere.bimcraft"})), access).unwrap_err();
        assert!(e.contains("would replace the project") && e.contains("File > Open"), "{e}");
    }
    // Let edit, the same agent's edit goes through.
    run(&mut s, &wall(5000), Access::Edit).unwrap();
    assert_eq!(walls(&s), 2);
}

#[test]
fn an_agent_may_ask_to_only_read_a_window_that_lets_it_edit() {
    let mut s = Session::new();
    let asked = |text: &str| parse(&line(json!({"token": T, "op": "execute", "command": text, "read_only": true})), T).unwrap();
    assert_eq!(asked("wall"), Request::Execute { command: "wall".into(), params: json!({}), read_only: true });
    assert!(
        matches!(parse(&line(json!({"token": T, "op": "execute", "command": "x", "read_only": "yes"})), T), Err(Error::Request(why)) if why.contains("`read_only`"))
    );
    let script = parse(&line(json!({"token": T, "op": "script", "text": "inspect", "read_only": true})), T).unwrap();
    assert_eq!(script, Request::Script { text: "inspect".into(), atomic: false, dry_run: false, read_only: true });
    let ask = Request::Execute { command: "wall".into(), params: json!({"points": [[0, 0], [4000, 0]]}), read_only: true };
    let e = run(&mut s, &ask, Access::Edit).unwrap_err();
    assert!(e.contains("read-only for agents") && e.contains("--read-only"), "it says the agent's side asked: {e}");
    assert_eq!(walls(&s), 0);
    assert!(run(&mut s, &script, Access::Edit).is_ok());
}

#[test]
fn a_dry_run_says_what_a_script_would_do_even_where_agents_only_read_and_keeps_nothing() {
    let mut s = Session::new();
    let text = "wall {\"points\": [[0, 5000], [4000, 5000]]}";
    let request = parse(&line(json!({"token": T, "op": "script", "text": text, "dry_run": true})), T).unwrap();
    assert_eq!(request, Request::Script { text: text.into(), atomic: false, dry_run: true, read_only: false });
    assert!(
        matches!(parse(&line(json!({"token": T, "op": "script", "text": "x", "dry_run": 1})), T), Err(Error::Request(why)) if why.contains("`dry_run`"))
    );
    for access in [Access::Edit, Access::Read] {
        let r = run(&mut s, &request, access).unwrap();
        assert_eq!(r["made"][0]["category"], "wall", "{r}");
        assert_eq!(walls(&s), 0, "nothing of it stays");
        assert!(s.history().is_empty());
    }
    // A line that would replace the project stops it as it stops any script.
    let replace = Request::Script { text: "new".into(), atomic: false, dry_run: true, read_only: false };
    assert!(run(&mut s, &replace, Access::Edit).unwrap_err().contains("replace the project"));
}

#[test]
fn the_file_keeps_a_port_and_a_token_for_the_user_alone_and_is_removed_only_by_its_own_window() {
    let dir = std::env::temp_dir().join(format!("bimcraft-bridge-{}", std::process::id()));
    let path = dir.join("nested").join(FILE);
    let entry = Entry { port: 40123, token: "abc".into() };
    assert_eq!(read_entry(&path), None, "no file");
    write_entry(&path, &entry).unwrap();
    assert_eq!(read_entry(&path), Some(entry.clone()));
    #[cfg(unix)]
    assert_eq!(std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&path).unwrap().permissions()) & 0o777, 0o600);
    // Another window has written it since: this one leaves it.
    let other = Entry { port: 40124, token: "def".into() };
    write_entry(&path, &other).unwrap();
    remove_entry(&path, &entry);
    assert_eq!(read_entry(&path), Some(other.clone()));
    remove_entry(&path, &other);
    assert_eq!(read_entry(&path), None);
    // What is not a file of ours reads as none.
    for text in ["", "{", "[]", r#"{"port": 70000, "token": "x"}"#, r#"{"port": 5, "token": ""}"#, r#"{"port": "5", "token": "x"}"#] {
        std::fs::write(&path, text).unwrap();
        assert_eq!(read_entry(&path), None, "{text}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
