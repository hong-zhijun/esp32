const fs = require('node:fs');
const path = require('node:path');
let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => { input += chunk; });
process.stdin.on('end', () => {
  try {
    const data = JSON.parse(input);
    const event = {
      time: new Date().toISOString(),
      event: data.hook_event_name,
      session: data.session_id,
      cwd: data.cwd,
      tool: data.tool_name,
      notification: data.notification_type
    };
    fs.appendFileSync(path.join(__dirname, 'events.jsonl'), JSON.stringify(event) + '\n');
  } catch (_) { /* Monitoring must not interrupt Claude Code. */ }
});
