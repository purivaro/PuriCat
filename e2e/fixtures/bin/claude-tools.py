#!/usr/bin/env python3
"""Stand-in for the Claude Code CLI when it is given an MCP server (see ./claude).

It is a real MCP client: it starts the server named in --mcp-config, lists its tools and
calls them, and prints the stream-json lines the real CLI prints around tool calls (shapes
captured from Claude Code 2.1.303). No network, no account.

What it calls comes from the user's last message:
- `PLAN:[{"name": ..., "arguments": {...}}, ...]` calls exactly those tools and answers with
  the results as JSON (used by the backend's integration test);
- anything else runs a fixed tour of the demo database (used by the end-to-end test).
"""
import json
import os
import subprocess
import sys

args = sys.argv[1:]


def after(flag):
    return args[args.index(flag) + 1] if flag in args else None


def emit(line):
    print(json.dumps(line, ensure_ascii=False), flush=True)


model = after("--model") or "stand-in-default"
prompt = sys.stdin.read()
record = os.environ.get("PURICAT_STAND_IN_DIR")
if record:
    # Under names of their own: the tool-less stand-in records the turns without tools.
    with open(os.path.join(record, "claude-tools.stdin"), "w") as f:
        f.write(prompt + "\n")
    with open(os.path.join(record, "claude-tools.args"), "w") as f:
        f.write("\n".join(args) + "\n")
    with open(os.path.join(record, "claude-tools.cwd"), "w") as f:
        f.write(os.getcwd() + "\n")

name, server = next(iter(json.loads(after("--mcp-config"))["mcpServers"].items()))
proc = subprocess.Popen([server["command"], *server["args"]], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
requests = 0


def rpc(method, params=None):
    global requests
    requests += 1
    message = {"jsonrpc": "2.0", "id": requests, "method": method}
    if params is not None:
        message["params"] = params
    proc.stdin.write(json.dumps(message) + "\n")
    proc.stdin.flush()
    line = proc.stdout.readline()
    if not line:
        emit({"type": "result", "subtype": "success", "is_error": True, "result": "The MCP server closed the connection"})
        sys.exit(1)
    return json.loads(line)


rpc("initialize", {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "stand-in", "version": "0"}})
proc.stdin.write('{"jsonrpc":"2.0","method":"notifications/initialized"}\n')
proc.stdin.flush()
tools = [tool["name"] for tool in rpc("tools/list")["result"]["tools"]]
emit({"type": "system", "subtype": "init", "tools": [f"mcp__{name}__{tool}" for tool in tools], "mcp_servers": [{"name": name, "status": "connected"}], "model": model})

question = prompt.rsplit("User:\n", 1)[-1].split("\n\nReply to the user's last message.")[0].strip()
if question.startswith("PLAN:"):
    plan = json.loads(question[5:])
    opening, closing = "Looking.", None
else:
    # PostgreSQL has schemas (the demo tables are in `shop`); MySQL has none.
    orders = "shop.orders" if "list_schemas" in tools else "orders"
    plan = [{"name": "list_tables", "arguments": {}}, {"name": "describe_table", "arguments": {"table": "orders"}}]
    if "run_select" in tools:
        plan.append({"name": "run_select", "arguments": {"sql": f"SELECT status, COUNT(*) AS n FROM {orders} GROUP BY status ORDER BY status"}})
        # The model tries to write through the reading tool: it must be refused.
        plan.append({"name": "run_select", "arguments": {"sql": f"DELETE FROM {orders} WHERE id = 2"}})
    if "ลบ" in question:
        plan.append({"name": "propose_sql", "arguments": {"sql": "DELETE FROM shop.orders", "reason": "ลบคำสั่งซื้อทั้งหมดตามที่ขอ"}})
    elif "หลายแถว" in question:
        # One row, twenty, one row: the second statement is over the demo connection's row limit.
        plan.append({"name": "propose_sql", "arguments": {"sql": "UPDATE shop.orders SET total_amount = total_amount + 1 WHERE id = 2;\nUPDATE shop.orders SET meta = meta WHERE id <= 20;\nUPDATE shop.orders SET total_amount = total_amount + 1 WHERE id = 3", "reason": "เพิ่มยอดของคำสั่งซื้อ id 2 และ 3 และบันทึก 20 รายการแรกซ้ำ"}})
    else:
        plan.append({"name": "propose_sql", "arguments": {"sql": f"UPDATE {orders}\nSET total_amount = total_amount + 1\nWHERE id = 1", "reason": "เพิ่มยอดของคำสั่งซื้อ id 1 ขึ้น 1 บาท"}})
    opening = "ขอดูโครงสร้างและข้อมูลก่อนนะครับ"
    closing = "ดูตาราง `orders` แล้วครับ ผมเสนอคำสั่งไว้ให้ตรวจด้านบน **ยังไม่ได้รัน** จนกว่าจะอนุมัติ"


def say(message_id, text):
    emit({"type": "stream_event", "event": {"type": "message_start", "message": {"id": message_id, "role": "assistant"}}})
    half = len(text) // 2
    for piece in (text[:half], text[half:]):
        emit({"type": "stream_event", "event": {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": piece}}})
    emit({"type": "assistant", "message": {"id": message_id, "role": "assistant", "content": [{"type": "text", "text": text}]}})


say("msg_1", opening)
results = []
for n, call in enumerate(plan):
    use_id = f"toolu_{n}"
    emit({"type": "assistant", "message": {"id": "msg_1", "role": "assistant", "content": [{"type": "tool_use", "id": use_id, "name": f"mcp__{name}__{call['name']}", "input": call["arguments"]}]}})
    reply = rpc("tools/call", {"name": call["name"], "arguments": call["arguments"]})
    if "result" in reply:
        result = {"name": call["name"], "arguments": call["arguments"], "isError": reply["result"]["isError"], "text": reply["result"]["content"][0]["text"]}
    else:
        result = {"name": call["name"], "arguments": call["arguments"], "isError": True, "text": reply["error"]["message"], "protocolError": True}
    results.append(result)
    emit({"type": "user", "message": {"role": "user", "content": [{"tool_use_id": use_id, "type": "tool_result", "content": [{"type": "text", "text": result["text"]}], "is_error": result["isError"]}]}})

if record:
    with open(os.path.join(record, "claude-tools.json"), "w") as f:
        json.dump({"tools": tools, "results": results}, f, ensure_ascii=False, indent=1)

answer = closing if closing is not None else json.dumps({"tools": tools, "results": results}, ensure_ascii=False)
say("msg_2", answer)
emit({"type": "result", "subtype": "success", "is_error": False, "result": answer, "num_turns": 2})
proc.stdin.close()
proc.wait(timeout=5)
