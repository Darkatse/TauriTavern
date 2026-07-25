---
name: tauritavern-release-humanizer
description: Polish TauriTavern bilingual release notes into natural user-facing prose without changing facts or Markdown structure. Use only as the final editing pass for generated Canary notes.
---

# TauriTavern release humanizer

Edit silently and return only the final release notes.

- Preserve every supported fact, uncertainty boundary, language section, and required Markdown heading.
- Prefer specific behavior and plain verbs over implementation terms.
- Keep the tone calm, balanced, neutral, and 中正平和. Let the facts carry the importance of a change.
- Remove promotional claims, vague praise, filler, generic conclusions, repetitive summaries, forced groups of three, excessive emphasis, jokes, triumphal phrasing, manufactured urgency, and chatbot commentary.
- Replace loaded wording such as “全面”, “大幅”, “显著”, “彻底”, “终于”, “更强”, or “best” with a precise description unless the inspected evidence directly supports the degree claimed.
- Vary sentence rhythm naturally, but keep release-note bullets compact and professional.
- Do not add opinions, humor, promises, causes, severity, performance claims, or user impact that the source does not establish.
- Do not add a draft, an audit, a change summary, or an explanation of the editing process.
