# Changelog

## [0.3.0](https://github.com/YuniqueUnic/brains/compare/v0.2.0...v0.3.0) (2026-10-10)


### Features

* Add FilesClient and SessionClient with CRUD operations for file management and session handling ([1d32a7f](https://github.com/YuniqueUnic/brains/commit/1d32a7fcf03cbf4f80d866b53299cce46c57e3e7))
* add network path copy functionality and improve editor status handling ([f252ca4](https://github.com/YuniqueUnic/brains/commit/f252ca4f6812518a6bedf8c9253bfe6b170fda9d))
* Add TypeScript definitions and Zod schemas for Brains API ([43922f9](https://github.com/YuniqueUnic/brains/commit/43922f9e10f54bb0850fb2649205ae4805875c93))
* **api-http:** generate the HTTP transport from api/brains.idl ([26fb7de](https://github.com/YuniqueUnic/brains/commit/26fb7de4d1a91687eec9a00d352432587f938aed))
* **e2e:** implement end-to-end tests for BrainStore functionality ([376c437](https://github.com/YuniqueUnic/brains/commit/376c4374edec689fbc508613928d73ec14bf81f8))
* enhance initial vault experience with template notes and AI integration ([d1b378d](https://github.com/YuniqueUnic/brains/commit/d1b378d58b3b0813bcd49ce8d660cc82bfcffbc0))
* enhance mobile layout with ARIA roles and tab synchronization ([24ae84f](https://github.com/YuniqueUnic/brains/commit/24ae84f2a9fa2997178cb3b31e397981ce72da07))
* Enhance sync error handling with detailed failure stages and improve hover preview responsiveness ([c6dda7b](https://github.com/YuniqueUnic/brains/commit/c6dda7b348b8a3be3b750c7f26e38bb031b62a75))
* enhance verification documentation and add new scripts for worker packaging and execution ([afe639b](https://github.com/YuniqueUnic/brains/commit/afe639bb63f4a07a04605a3f8431ad46dcccc648))
* implement theme provider with light/dark/system modes and i18n support ([91acefd](https://github.com/YuniqueUnic/brains/commit/91acefda41ad418ef272a9c0c2c3790a6ee9dbbc))
* implement workspace dialogs for file and folder management ([f60bac9](https://github.com/YuniqueUnic/brains/commit/f60bac99e0c340f92aae5dfbfe80e309756883e1))
* Revamp UI/UX for Knowledge Workbench ([1f6af8c](https://github.com/YuniqueUnic/brains/commit/1f6af8c124781852cecdc97391d99f46a7098003))
* support standalone local libraries and remote collaboration in bsx ([9148a61](https://github.com/YuniqueUnic/brains/commit/9148a616f357f4f72fc8d632b70a2a6f6f34e541))
* **ui:** add HoverMenu and SliderRow components for enhanced UI interactions ([39bcc77](https://github.com/YuniqueUnic/brains/commit/39bcc7701f58a39ed66d2dba6f41ec4a40cde6de))
* use Turso with optional Tantivy search in native libraries ([c628990](https://github.com/YuniqueUnic/brains/commit/c6289905733c0828f17bedf15762a81c560661a7))
* **web:** render metadata and share resizable reference previews ([79e13b1](https://github.com/YuniqueUnic/brains/commit/79e13b17291f6e05ac6589bea45aab3c83a305ac))
* **workspace:** enhance clipboard import functionality and improve file movement logic ([39bcc77](https://github.com/YuniqueUnic/brains/commit/39bcc7701f58a39ed66d2dba6f41ec4a40cde6de))


### Bug Fixes

* **api:** restore HEAD metadata and validate merged business workflows ([15c335a](https://github.com/YuniqueUnic/brains/commit/15c335ac7c1dd8c69f84ae0e4e72f55ad52db130))
* **ci:** install components from the pinned Rust toolchain ([1058ce8](https://github.com/YuniqueUnic/brains/commit/1058ce81c0d7f01de341cf1d577b1cba8e89ae0e))
* **deps:** update the shared lockfile for E2E TypeScript 7 ([6240036](https://github.com/YuniqueUnic/brains/commit/62400361ac3b3910e7c3785bbf965cf30a9a2242))
* **domain:** decode escaped values in signed pagination cursors ([5ca0224](https://github.com/YuniqueUnic/brains/commit/5ca02242d080f2e1f595f19f18e1ef233f27d2d2))
* **format:** parse YAML metadata consistently and refresh old indexes ([ff4ad5b](https://github.com/YuniqueUnic/brains/commit/ff4ad5b8f3391e58d268afee957a5f0f33d29c83))
* **http:** align generated SDKs with patch and representation contracts ([a267b54](https://github.com/YuniqueUnic/brains/commit/a267b54d1613070d20dada0f142090c57c4b243e))
* **http:** preserve typed text-transfer errors with compiler-only builtins ([4c2bd87](https://github.com/YuniqueUnic/brains/commit/4c2bd871bdf72a03d0198444d0cf49335c0a3cb6))
* **i18n:** update English and Chinese translations for improved clarity ([39bcc77](https://github.com/YuniqueUnic/brains/commit/39bcc7701f58a39ed66d2dba6f41ec4a40cde6de))
* normalize indexed relationships before SQLite and D1 updates ([e3ffcdf](https://github.com/YuniqueUnic/brains/commit/e3ffcdfb973a7b339b64855b283ca4e5f2576a4c))
* repair preview resizing, tag search and file navigation ([b0c4a44](https://github.com/YuniqueUnic/brains/commit/b0c4a44b0c1e5e8b62cfb48449cb3e73f72e1c15))
* **sdk:** regenerate with xidlc 0.96 and simplify the gen pipeline ([187fc58](https://github.com/YuniqueUnic/brains/commit/187fc589ac279bd669cac1d7aace7c5349033cf0))
* update error handling for domain errors ([f60bac9](https://github.com/YuniqueUnic/brains/commit/f60bac99e0c340f92aae5dfbfe80e309756883e1))
* update selectors for file item interactions in screenshots script ([b3537f5](https://github.com/YuniqueUnic/brains/commit/b3537f5dddcff8b7f86ea9c71c1c7f85715e259b))
* **upload:** improve upload conflict handling and streamline upload process ([39bcc77](https://github.com/YuniqueUnic/brains/commit/39bcc7701f58a39ed66d2dba6f41ec4a40cde6de))
* **web:** make the public demo preserve files and navigation state ([fbd427c](https://github.com/YuniqueUnic/brains/commit/fbd427c48c3329c2ef16d694ef0d594e50269b44))
* **web:** repair source editing and keyboard search workflows ([7ef04e8](https://github.com/YuniqueUnic/brains/commit/7ef04e87bcb3fcfbfeeb0221b6a7f68f107c46fe))
