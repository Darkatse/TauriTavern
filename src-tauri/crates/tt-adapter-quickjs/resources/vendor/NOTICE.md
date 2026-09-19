# Embedded script kit

These minified ESM bundles are compiled into `tt-adapter-quickjs` and exposed as
`@tauritavern/kit/*`. Do not edit generated files by hand.

| Package | Version | Entry | SHA-256 |
| --- | --- | --- | --- |
| dayjs | 1.11.21 | `dayjs/esm/index.js` | `3b1171b77d349e2544441c95f5a0a407fb10128bfd0a9ef7097f6eee84b1412d` |
| es-toolkit | 1.50.0 | `es-toolkit/dist/index.mjs` | `8f5d343e94352c336e6920830f6f910d92917047573eaa2a125200888ed4ccf6` |
| fast-xml-parser | 5.10.1 | `fast-xml-parser/src/fxp.js` | `39d14f1585195faa6efe189ecd2b2eca01d82dba4512cdf7d43b28368f278296` |
| marked | 18.0.9 | `marked/lib/marked.esm.js` | `3e55e94affb0b0220156dd6f4f375e069768e0d66aef32c566f523813f308486` |
| papaparse | 5.6.0 | `papaparse/papaparse.min.js` | `e803a3caa12476f70951fd6341b8f44ce2b8bd488bce12627ba69428cbb65fec` |
| slugify | 1.6.9 | `slugify/slugify.js` | `1991650cbfd13285781a9f43c6090e9ee429f21fd5f49d21073abeefe61073a3` |

The sources were bundled for neutral ES2020 ESM with esbuild 0.28.2, then
compressed without identifier mangling by Terser 5.50.0 and normalized to one
trailing newline. The fast-xml-parser build pins `is-unsafe` 2.0.0. All six
upstream packages are MIT-licensed.
