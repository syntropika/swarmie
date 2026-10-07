//! Public schemas shared by the daemon MCP bridge and native custom-tool integrations.
use serde_json::{Value, json};

pub fn tools() -> Vec<Value> {
    let target = json!({"type":"object","properties":{"harness":{"type":"string"},"model":{"type":"string"},"cwd":{"type":"string"}},"additionalProperties":false});
    let string = || json!({"type":"string","minLength":1,"maxLength":65536});
    let definitions = [
        (
            "peers.spawn",
            "Create an independent agent session, optionally delivering an initial prompt",
            json!({"cwd":string(),"harness":string(),"model":string(),"text":string()}),
            vec!["cwd"],
        ),
        (
            "peers.read",
            "Read actual runtime events from a peer session",
            json!({"sessionId":string(),"afterSequence":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":4096}}),
            vec!["sessionId"],
        ),
        (
            "peers.send",
            "Deliver a prompt to a peer; busy or interactive sessions receive queued work",
            json!({"sessionId":string(),"text":string()}),
            vec!["sessionId", "text"],
        ),
        (
            "fanout.start",
            "Compare one prompt across up to eight independent model sessions",
            json!({"cwd":string(),"text":string(),"targets":{"type":"array","items":target,"minItems":1,"maxItems":8},"timeoutSeconds":{"type":"integer","minimum":1,"maximum":3600}}),
            vec!["cwd", "text", "targets"],
        ),
        (
            "fanout.status",
            "Inspect durable model comparison results and native session identifiers",
            json!({"id":string()}),
            vec!["id"],
        ),
        (
            "fanout.cancel",
            "Interrupt the active native sessions in a model comparison",
            json!({"id":string()}),
            vec!["id"],
        ),
    ];
    definitions.into_iter().map(|(command,description,properties,required)|json!({"name":command.replace('.',"_"),"command":command,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})).collect()
}
