# Token Monitor 源码研究：ESP32 AI 状态屏

研究日期：2026-10-26。项目：https://github.com/Javis603/token-monitor

## 结论

适合参考其 Claude 额度采集适配器，不需要为小屏部署完整 Electron 桌面应用。
已阅读额度模块、隔离探测模块、字段归一化、相关测试与 MIT 许可证。
这次是静态源码研究，未运行上游测试，未读取本机凭据或调用账号额度接口。
完整仓库下载遇到超时，关键源码单独保存在 token-selected/ 下。

## 已确认的实现

- `src/shared/providers/claude/limits.js` 的 `fetchClaudeLimits()`：显式配置 Web sessionKey 时优先 Web，否则尝试 Claude Code OAuth，再按错误类型回退 CLI。
- OAuth 查询地址：`https://api.anthropic.com/api/oauth/usage`。它是项目使用的服务端接口，不应视为有稳定兼容保证的公共 API。
- `mapClaudeUsageToProvider()` 将 `five_hour.utilization`、`seven_day.utilization` 映射为已用百分比，将各窗口 `resets_at` 映射为重置时间。缺失值不能当作零。
- 凭据支持文件、Windows Credential Manager 等来源；OAuth token 到期处理与账号身份确认独立存在。CLI 登录不保证与桌面端是同一个账号。
- CLI 回退先判断版本；其版本门槛是项目自己的兼容策略，不是官方支持承诺。打印路径禁用工具、Hooks 和会话持久化，并隔离工作目录，避免额度探测产生虚假的任务事件。
- 检查 CLI 输出中真实的 session quota 后才接受结果，退出码为零不等于拿到了额度。
- 归一化状态区分未配置、未授权、来源限流、不可用等。请求限流不等于账号额度耗尽。
- 相关测试涵盖 Windows CLI、凭据管理器、不同组织账号隔离、Cookie 更新及错误回退；测试使用的模拟响应不能证明我们本机能连接成功。
- MIT 允许复用；复制代码需保留 LICENSE 和版权声明。额度模块依赖多个共享模块，不能只复制单个 JS 文件就直接运行。

## 我们的实施建议

1. 先实现一次性只读额度探测，输出来源、时间、两组百分比和重置时间；凭据不进入日志或发送到 ESP32。
2. 与桌面端用量页对照账号及读数，确认五小时/每周窗口语义。没有成功数据前显示未连接，不填演示百分比。
3. 验证通过后以 60 秒为正常刷新间隔，串行请求；遇到来源限流按响应退避，错误时保留旧数据并标记过期。时间经过不代表已成功刷新。
4. 继续使用本机已经实测有效的 Hooks 获取任务事件。Stop 显示“本轮结束/待查看”，不能作为业务任务成功的证明。PermissionRequest 尚未实测。
5. 为 Claude/Codex 定义一致的 provider 数据格式，ESP32 只负责渲染 640×172 页面。当前仅研究 Claude 适配器，未验证 Codex 接入。
6. 不复用多设备云同步、计费估算、Electron 窗口、充值和额外消费额度等无关功能。

## 下一步验证范围

一次额度探测 → 对照桌面用量页 → 每分钟刷新与失败处理 → 接入横屏界面。
本次没有修改已烧录固件，没有安装 Token Monitor，也没有修改 Claude 登录状态。
