# Provider logos

Checked-in monochrome SVG marks for Claude, ChatGPT/OpenAI and DeepSeek, from LobeHub Icons:

- Repository: https://github.com/lobehub/lobe-icons
- Revision: `8b7bbc433d38437fa2b0cca58ae896917162b812`
- Files: `packages/static-svg/icons/{claude,openai,deepseek}.svg`; `openai.svg` is stored locally as `chatgpt.svg`.
- Upstream license: [LICENSE.upstream](LICENSE.upstream)

`firmware/tools/generate-provider-logos.cjs` uses Sharp to generate 20×20 alpha masks
in `firmware/ai-status-screen/provider_logos.h`. The three masks occupy 1,200 bytes;
LVGL recolors them using the corresponding provider's existing screen color.
The original SVG files remain here so the output can be regenerated.
