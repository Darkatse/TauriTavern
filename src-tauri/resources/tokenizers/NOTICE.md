# Bundled tokenizer sources

TauriTavern embeds these tokenizer files, gzip-compressed, so that the models they serve count without a download. Each is the file MikTik's `scripts/fetch-test-assets.sh` pins by the SHA-256 of the uncompressed file. The files belong to their owners and are distributed under the owners' terms.

| File | Serves | Origin | Terms |
| --- | --- | --- | --- |
| `claude.json.gz` | Claude | SillyTavern `src/tokenizers/claude.json` at `7e8663cd` | Anthropic's |
| `deepseek.json.gz` | DeepSeek V3 and V4 | SillyTavern-Tokenizers `deepseek.json.gz` at `798bcd82` | DeepSeek's |
| `gemma3.model.gz` | Gemma 3, Gemini | `unsloth/gemma-3-1b-it` at `5b11413a` | Gemma Terms of Use |
| `glm.json.gz` | GLM 4.5 to 5.3 | `zai-org/GLM-5.2` at `cf457fa7` | MIT |
| `kimi.tiktoken.gz` | Kimi K2 to K3.1 | `moonshotai/Kimi-K2-Instruct` at `fd1984e2` | Modified MIT |
| `qwen2.json.gz` | Qwen up to 3, Grok | SillyTavern-Tokenizers `qwen2.json.gz` at `798bcd82` | Apache-2.0 |
| `qwen3.5.json.gz` | Qwen 3.5 and later | `Qwen/Qwen3.5-0.8B` at `2fc06364` | Apache-2.0 |

MiniMax's tokenizer is not bundled: TauriTavern downloads it on first use, so it does not redistribute the file under MiniMax's license.
