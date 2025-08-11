# The Balance (中文文档)

[![MIT 许可证](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![欢迎 PR](https://img.shields.io/badge/PRs-welcome-brightgreen.svg?style=flat-square)](http://makeapullrequest.com)

一个智能 API 网关，用于将请求路由到 AI 提供商。它基于 Rust 构建，并在 Cloudflare Workers 上运行，为管理和使用多个提供商的 API 密钥提供了一个弹性和可观察的接口。

## 概述

本项目是一个基于 Rust 的 monorepo，结构为 Cargo 工作区。其核心目标是作为一个高可用的 API 网关，管理 AI 提供商（目前主要基于 Google Gemini）的 API 密钥池，并智能地路由传入的请求。

项目提供以下几个不同的接口：

1.  **AI 网关 API (`/api/*`)**: worker 的主要功能是一个复杂的反向代理，能智能处理不同类型的 API 请求。其行为会根据环境（生产 vs. 本地开发）进行调整。

    网关支持三种不同的模式：

    *   **A) 兼容 OpenAI 的聊天 API (`/api/compat/chat/completions`)**
        *   **生产环境:** 将 OpenAI 格式的请求直接转发到 Cloudflare AI 网关，依赖其将请求转换为原生提供商的 API 格式。
        *   **本地开发:** worker 内置的转换层会将 OpenAI 请求转换为原生的 Google Gemini 格式，然后再发送到提供商的实际端点，并对响应进行反向转换。

    *   **B) 兼容 OpenAI 的嵌入 API (`/api/compat/embeddings`)**
        *   **生产环境 & 本地开发:** worker 的内置转换层在两种环境中都会激活。它会将 OpenAI 请求体转换为原生 Gemini 格式，构建相应的原生提供商路径，然后发送请求，并对响应进行反向转换。

    *   **C) 特定提供商的 API 代理 (`/api/{provider}/*`)**
        *   此模式允许客户端直接使用提供商的原生 API 和 SDK。网关会拦截这些原生请求，将密钥池中状态最佳的 API 密钥注入到认证头中，然后转发请求。
        *   **生产环境:** 将新认证的请求转发到 Cloudflare AI 网关的特定提供商 API 端点。
        *   **本地开发:** 将新认证的请求直接转发到提供商的实际端点（例如 `generativelanguage.googleapis.com`）。

2.  **密钥管理 UI**: 一个用于管理 API 密钥池的 Web 界面。
3.  **密钥管理 API**: 用于 UI 或程序化管理的端点：
    *   `POST /api/keys/add/{provider}`: 为特定提供商添加一个或多个密钥。
    *   `GET /api/keys/{id}/coolings`: 检索单个密钥的详细冷却状态。
4.  **管理 API (`/test/run-cleanup/*`)**: 手动触发后台进程，清理永久失效但仍处于活动状态的密钥。

项目还包括一个命令行工具：

*   **同步 CLI (`sync-cli`)**: 用于将 API 密钥从一个 The One Balance 实例同步到另一个实例。

## 架构

系统设计旨在实现高可用性和故障恢复能力，重点关注智能密钥管理和性能。

### 密钥生命周期、负载均衡和高可用性

网关的核心优势在于其对 API 密钥池的动态管理。

1.  **密钥检索和健康评分**: 收到请求后，系统会检索并按健康分（延迟、成功率、连续失败次数）对 `active` 状态的密钥进行排序。
2.  **故障转移循环**: 系统会按排序列表尝试密钥，如果失败则自动、透明地用下一个密钥重试。
3.  **错误分析和状态变更**:
    *   **瞬时错误**: 使用相同密钥重试。
    *   **冷却中的密钥 (例如，速率限制)**: 将密钥置于临时冷却状态，并尝试下一个。
    *   **无效密钥错误**: 将密钥标记为 `blocked`，并尝试下一个。
4.  **双缓存设计**:
    *   **主缓存**: 存储所有健康的密钥，定期从 D1 数据库更新。
    *   **冷却缓存**: 临时拉黑最近失败的密钥，为故障转移循环提供即时反馈。
5.  **清理机制**:
    *   **自动清理**: 定期运行的后台进程会验证并删除永久无效的密钥。
    *   **手动管理**: UI 允许手动删除任何密钥。

### 高级超时机制

多层超时策略确保可靠性：

1.  **整体请求超时**: 整个处理过程的顶层超时（默认 25 秒）。
2.  **单次尝试超时**: 每次使用单个密钥尝试的较短超时（默认 10 秒）。
3.  **动态超时计算**: 每次尝试前，系统会计算剩余时间，并使用两者中较小的值作为当前尝试的超时时间。

### 技术文档

更多详细信息，请参阅 `/docs` 目录中的文档：

*   [`HYBRID_ORM_PATTERN.md`](./docs/HYBRID_ORM_PATTERN.md): 解释了自定义的 ORM 式系统。
*   [`two-cache-design.md`](./docs/two-cache-design.md): 描述了缓存策略。

## 系统要求

*   [Rust](https://www.rust-lang.org/tools/install) (最新稳定版)
*   [Node.js](https://nodejs.org/en/) (v18+)
*   [pnpm](https://pnpm.io/installation)

## 开始使用

### 初始设置

首先，安装 `just`:
```bash
cargo install just
```

1.  **创建环境文件**:
    ```bash
    cd crates/theone-balance
    cp .env.example .env
    cd ../..
    ```
    在新创建的 `.env` 文件中填入你的机密信息。

2.  **配置 `wrangler.jsonc.tpl`**:
    在此文件中更新非机密变量。

3.  **安装依赖**:
    ```bash
    just install-all
    ```

4.  **推送机密信息**:
    ```bash
    just secrets:push
    ```
完成这些步骤后，你就可以从根目录使用 `just` 命令了。

*   **本地开发**: 无需其他设置。
*   **生产部署**: 在 Cloudflare 仪表板中创建一个 AI 网关，并启用认证以获取 `AI_GATEWAY_TOKEN`。Worker 将被自动创建。

## 项目使用

这是一个如何将此网关配置到 `claude-code-route` 的示例：

```json
{
  "name": "cloudflare-ai-rust",
  "api_base_url": "https://xx.xxx.workers.dev/api/compat/chat/completions",
  "api_key": "AUTH_KEY",
  "models": ["google-ai-studio/gemini-2.5-pro", "google-ai-studio/gemini-2.5-flash"],
   "transformer": {
     "use": ["cloudflare-payload-fixer"]
   }
}
```
`cloudflare-payload-fixer` 转换器文件位于 `crates/claude-code-router/transformers/payload-fixer.js`，它修复了 Gemini 返回空内容和工具调用无法继续的问题。

## 构建和部署

### 工作区编排器 (`justfile`)

根目录的 `justfile` 是主要接口。使用 `just -l` 列出所有命令。

-   `just dev`: 启动本地开发服务器。
-   `just deploy`: 将 worker 部署到 Cloudflare。
-   `just migrate`: 运行本地数据库迁移。
-   `just migrate-remote`: 运行生产数据库迁移。
-   `just secrets-push`: 将 `.env` 中的机密信息推送到 Cloudflare。
-   `just build-cli`: 编译 `sync-cli` 工具。
-   `just sync`: 运行 `sync-cli`。

### 核心 Worker 逻辑 (`crates/theone-balance`)

Worker 逻辑位于 `crates/theone-balance`。这里配置了 `wrangler`、`pnpm` 和 `drizzle-kit`。

-   `pnpm dev`: 启动本地开发服务器。
-   `pnpm migrate`: 运行数据库迁移。
-   `pnpm deploy`: 部署 worker。

## 功能标志

`crates/theone-balance/Cargo.toml` 定义了功能标志：

-   `default`: 编译 Cloudflare Worker 库。
-   `sync_cli`: 编译 `sync-cli` 二进制文件。

## 测试

### 当前测试策略

1.  **theone-balance (核心 Worker)**:
    -   `tests/integration_test.rs` 中的集成测试用于测试核心业务逻辑。
    -   目前被忽略 (`#[ignore]`)，因为它需要连接到活动的 D1 数据库。

### 测试待办事项和路线图

-   [ ] 将测试与实时服务解耦，使用模拟数据库。
-   [ ] 使用 `axum-test` 实现 HTTP API 端点测试。
-   [ ] 使用 `assert_cmd` 为 CLI 添加专用的测试套件。
-   [ ] 扩展故障转移、重试和指标更新的测试覆盖范围。

### 路线图

-   [ ] **其他提供商的测试和验证**: 添加并验证对其他 AI 提供商的支持。
-   [ ] **减小二进制文件大小**: 优化最终的二进制文件以减小其体积。
-   [ ] **支持到原生端点的原始套接字**: 实现到原生提供商端点的直接原始套接字连接以降低延迟。
-   [ ] **为 `claude-code-route` 添加转换器**: 实现请求/响应转换器以解决特定于提供商的问题，例如修复 Gemini 的停止序列问题。
-   [ ] **从其他来源同步密钥**: 扩展 `sync-cli` 以支持从各种来源导入密钥。
-   [ ] **跨提供商的负载均衡**: 根据提示智能实现跨不同 AI 提供商的负载均衡。
-   [ ] **优化超时设置**: 微调超时机制以获得更好的性能和可靠性。

### 临时测试

#### 本地服务器测试
```bash
# 兼容 OpenAI 的聊天
curl http://localhost:8087/api/compat/chat/completions \
 -H "Content-Type: application/json" \
 -H "cf-aig-authorization: Bearer local-cf-api-token" \
 -H "Authorization: Bearer local-auth-key" \
 -d '{
   "model": "google-ai-studio/gemini-2.5-pro",
   "messages": [{"role": "user", "content": "你好！"}]
 }'

# 兼容 OpenAI 的嵌入
curl "http://localhost:8087/api/compat/embeddings" \
 -H "Content-Type: application/json" \
 -H "Authorization: Bearer local-auth-key" \
 -H "cf-aig-authorization: Bearer locl-cf-api-token" \
 -d '{"input": "这是一个用于嵌入的测试句子。", "model": "google-ai-studio/text-embedding-004"}'

# 特定提供商的 Gemini 格式
curl -X POST "http://localhost:8087/api/google-ai-studio/v1beta/models/text-embedding-004:batchEmbedContents" \
 -H "Content-Type: application/json" \
 -H "cf-aig-authorization: Bearer local-cf-api-token" \
 -H "Authorization: Bearer local-auth-key" \
 -d '{"requests": [{"model": "models/text-embedding-004", "content": {"parts": [{"text": "这是一个原生 Gemini API 的测试。"}]}}]}'

# 测试清理活动但失败的密钥
curl -X POST -H "Authorization: Bearer local-auth-key" http://localhost:8087/test/run-cleanup/google-ai-studio

# 向本地数据库添加密钥
curl -X POST http://localhost:8087/api/keys/add/google-ai-studio \
 -H "Authorization: Bearer local-auth-key" \
 -H "Content-Type: text/plain" \
 --data "keys=KEY1,KEY2,KEY3"
```

#### Worker 测试
```bash
# 特定提供商的 Gemini 格式
curl -X POST "https://xx.xxx.workers.dev/api/google-ai-studio/v1beta/models/text-embedding-004:batchEmbedContents" \
 -H "Content-Type: application/json" \
 -H "cf-aig-authorization: Bearer YOUR_AIG_TOKEN" \
 -H "x-goog-api-key: YOUR_AUTH_KEY" \
 -d '{"requests": [{"model": "models/text-embedding-004", "content": {"parts": [{"text": "这是一个测试。"}]}}]}'

# 兼容 OpenAI 的嵌入
curl "https://xx.xxx.workers.dev/api/compat/embeddings" \
 -H "Content-Type: application/json" \
 -H "Authorization: Bearer YOUR_AUTH_KEY" \
 -H "cf-aig-authorization: Bearer YOUR_AIG_TOKEN" \
 -d '{"input": "这是一个测试句子。", "model": "google-ai-studio/text-embedding-004"}'

# 兼容 OpenAI 的聊天
curl "https://xx.xxx.workers.dev/api/compat/chat/completions" \
 -H "Content-Type: application/json" \
 -H "Authorization: Bearer YOUR_AUTH_KEY" \
 -H "cf-aig-authorization: Bearer YOUR_AIG_TOKEN" \
 -d '{"model": "google-ai-studio/gemini-2.5-pro", "messages": [{"role": "user", "content": "你好！"}]}'
```

## 调试

### 远程 Worker 日志
-   在 `crates/theone-balance` 目录中运行 `npx wrangler tail`
-   或运行 `just tail`
-   或登录 Cloudflare 仪表板查看 worker 日志

### 本地日志
-   日志会显示在 `just dev` 控制台中。

## 致谢

本项目灵感来源于 [one-balance](https://github.com/glidea/one-balance)。

## 许可证

本项目采用 MIT 许可证。详见 [LICENSE](LICENSE) 文件。
