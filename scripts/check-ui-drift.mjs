#!/usr/bin/env node
/**
 * Fail when the app hand-rolls something the shared design system already ships.
 *
 * This catches what no other gate can see. A hand-written card compiles,
 * renders, type-checks and passes its tests; it is only wrong once a human
 * opens it on a real display and sees that it does not match the rest of the
 * app. Its sibling gate (check-design-craft.mjs) catches craft defects in what
 * the app DID compose; this one catches the app composing nothing at all where
 * the package ships the answer.
 *
 * `canonical-app-shape.md` makes this gate binding on every full-stack app.
 *
 * THREE RULES:
 *   1. vendored-copy — a local component whose name matches one @poodle64/ui
 *      ships, and which does not delegate to it.
 *   2. hand-rolled-page-title — a route writing its own <h1> instead of
 *      composing the shared PageHeader.
 *   3. surface-brief-divergence — a route composing a package component its
 *      surface brief does not name. Inert until the app grows
 *      `docs/product/surfaces/`; surface briefs are optional, so an app
 *      without them never sees this rule fire.
 *
 * Usage:  node scripts/check-ui-drift.mjs [--json] [--baseline]
 * Exit:   0 clean · 1 new drift · 2 could not run
 */

import { readdirSync, readFileSync, existsSync, statSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

// Anchor to the frontend package, not the process cwd. pre-commit runs its
// hooks from the repo root, but `pnpm lint:drift` runs from the frontend — the
// script must resolve the same paths either way. The script lives at
// <frontend>/scripts/, so the frontend root is its parent.
//
// Where that frontend sits is the one thing that differs by archetype, so it is
// DERIVED rather than assumed. A full-stack app nests it at `frontend/`, a name
// the canonical shape mandates, so the basename is a reliable signal. A Tauri
// desktop app has no server to sit beside: its SvelteKit app IS the repo root,
// and there is no enclosing directory to step up into. Hardcoding the nested
// case made every desktop app hand-edit this line, which forks a factory file
// over one constant — exactly what an extension point exists to prevent.
const FRONTEND_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const REPO_ROOT =
	path.basename(FRONTEND_ROOT) === 'frontend' ? path.resolve(FRONTEND_ROOT, '..') : FRONTEND_ROOT;
// Where to tell someone to run pnpm: `frontend` when nested, `.` when the
// frontend is the repo root.
const INSTALL_DIR = path.relative(REPO_ROOT, FRONTEND_ROOT) || '.';
const UI_DIST = path.join(FRONTEND_ROOT, 'node_modules/@poodle64/ui/dist/components/ui');
const SRC = path.join(FRONTEND_ROOT, 'src');
const ROUTES = path.join(SRC, 'routes');
const BASELINE = path.join(FRONTEND_ROOT, '.ui-drift-baseline.json');
const SURFACES_DIR = path.join(REPO_ROOT, 'docs/product/surfaces');

if (!existsSync(UI_DIST)) {
	console.error(
		`@poodle64/ui not installed at ${path.relative(FRONTEND_ROOT, UI_DIST)} — run pnpm install in ${INSTALL_DIR} first.`
	);
	process.exit(2);
}

const shipped = new Set(
	readdirSync(UI_DIST).filter((d) => statSync(path.join(UI_DIST, d)).isDirectory())
);

/** kebab-case a PascalCase component name, to compare against a package subpath. */
const kebab = (name) => name.replace(/([a-z0-9])([A-Z])/g, '$1-$2').toLowerCase();

const kebabToPascal = (k) =>
	k
		.split('-')
		.map((s) => s[0].toUpperCase() + s.slice(1))
		.join('');

function walk(dir, out = []) {
	if (!existsSync(dir)) return out;
	for (const entry of readdirSync(dir, { withFileTypes: true })) {
		const full = path.join(dir, entry.name);
		if (entry.isDirectory()) walk(full, out);
		else out.push(full);
	}
	return out;
}

const files = walk(SRC);
const rel = (f) => path.relative(FRONTEND_ROOT, f);
const findings = [];

// ---------------------------------------------------------------------------
// 1. A local component with the same name as one the package ships.
//
// `sveltekit-frontend.md` forbids this outright, and the reason is not
// tidiness: a vendored copy cannot receive an upstream fix. It also drifts —
// the copy grows a prop, the shared one grows a different one, and the two
// diverge silently because nothing compares them.
//
// `components/ui/` is excluded: those are this app's own shadcn primitives for
// things the package genuinely does not ship (chart, form, sheet, sidebar).
//
// A local file that IMPORTS the shipped component of the same name is excluded
// too, and that distinction is the rule rather than a hole in it: a DELEGATING
// composition is the opposite of a fork — a thin wrapper that adapts the
// package's API to a call shape this app repeats. A genuine fork cannot pass
// the test, because it does not import the thing it forked.
// ---------------------------------------------------------------------------
for (const f of files.filter((f) => f.endsWith('.svelte'))) {
	if (f.includes(`${path.sep}components${path.sep}ui${path.sep}`)) continue;
	const name = path.basename(f, '.svelte');
	if (!shipped.has(kebab(name))) continue;
	const src = readFileSync(f, 'utf8');
	if (new RegExp(`from\\s+['"]@poodle64/ui/${kebab(name)}['"]`).test(src)) continue;
	findings.push({
		rule: 'vendored-copy',
		file: rel(f),
		detail: `local ${name} duplicates @poodle64/ui/${kebab(name)}; import the shipped one`
	});
}

// ---------------------------------------------------------------------------
// 2. A route page that writes its own <h1> instead of composing PageHeader.
//
// Each hand-rolled title looks perfectly reasonable in its own file; the
// divergence is only visible across files, which is why a human never catches
// it and a script always does.
//
// No route type is exempt here, on purpose. A route that legitimately owns its
// own title treatment (an immersive surface rendering without the workbench
// shell) banks the finding in .ui-drift-baseline.json; a whole class of such
// routes is argued as an app-local exception with its reason recorded — never
// inherited from another app.
// ---------------------------------------------------------------------------
for (const f of files.filter((f) => path.basename(f) === '+page.svelte')) {
	const src = readFileSync(f, 'utf8');
	// Deliberately NOT "…and does not import PageHeader": a page that swapped its
	// PageHeader back for a raw <h1> while leaving the now-unused import behind
	// would satisfy that condition. PageHeader emits the page's <h1> itself, so a
	// route writing its own is wrong either way — it has abandoned the shared
	// treatment, or it has shipped two h1s.
	if (/<h1[\s>]/.test(src)) {
		findings.push({
			rule: 'hand-rolled-page-title',
			file: rel(f),
			detail: 'writes its own <h1>; compose PageHeader so every route shares one title treatment'
		});
	}
}

// ---------------------------------------------------------------------------
// 3. A route composing a package component its surface brief does not name.
//
// A surface brief records the presentation decision for one route, naming the
// composed vocabulary it reasoned its way to. Without this check a brief can be
// edited, or a route can quietly grow a new component, and the two drift with
// nothing noticing.
//
// Inert until the app grows `docs/product/surfaces/`. Surface briefs are
// optional, so an app that does not keep them never sees this rule fire — but
// an app that adopts them gets the gate without re-deriving it.
//
// Primitives are free: a brief is a presentation decision, and reaching for a
// Button is not one. Only a COMPOSED component counts.
// ---------------------------------------------------------------------------
const PRIMITIVES = new Set([
	'alert',
	'alert-dialog',
	'avatar',
	'badge',
	'button',
	'card',
	'checkbox',
	'command',
	'data-table',
	'dialog',
	'dropdown-menu',
	'input',
	'input-group',
	'label',
	'password-input',
	'popover',
	'progress',
	'select',
	'separator',
	'skeleton',
	'sonner',
	'switch',
	'table',
	'tabs',
	'textarea',
	'tooltip'
]);

const surfaceFiles = existsSync(SURFACES_DIR)
	? readdirSync(SURFACES_DIR).filter((f) => f.endsWith('.md') && f !== 'README.md')
	: [];

// route -> +page.svelte file. A route group is a parenthesised directory name
// (`(protected)`, `(admin)`) that organises files on disk without appearing in
// the URL, so it has to be stripped before a brief's `route:` (which names the
// URL) can be matched against a filesystem path.
const routeToPageFile = new Map();
for (const f of files.filter((f) => path.basename(f) === '+page.svelte')) {
	const relDir = path.relative(ROUTES, path.dirname(f));
	const segments = relDir.split(path.sep).filter((s) => s && !/^\(.*\)$/.test(s));
	routeToPageFile.set('/' + segments.join('/'), f);
}

/** Pull the named `## Heading` section's body out of a brief's markdown. */
function briefSection(md, heading) {
	const lines = md.split('\n');
	const start = lines.findIndex((l) => l.trim() === `## ${heading}`);
	if (start === -1) return '';
	let end = lines.findIndex((l, idx) => idx > start && /^## /.test(l));
	if (end === -1) end = lines.length;
	return lines.slice(start + 1, end).join('\n');
}

/**
 * The brief's declared vocabulary: shipped component names named anywhere in
 * `## The decision`, not only in a closing "Composed vocabulary:" line. A brief
 * reasons about a component inline well before it reaches that summary
 * sentence, and relying on the sentence alone would miss a brief that names a
 * component only in prose.
 */
function declaredVocabulary(decisionSection) {
	const declared = new Set();
	const backtickRe = /`([^`]+)`/g;
	let m;
	while ((m = backtickRe.exec(decisionSection))) {
		for (const candidate of m[1].split(',')) {
			const k = kebab(candidate.trim());
			if (shipped.has(k)) declared.add(k);
		}
	}
	return declared;
}

/**
 * Every COMPOSED `@poodle64/ui` component a source file imports. Skips
 * `import type` — a type import composes nothing on screen, and several shipped
 * directories export a type of the same name.
 */
function composedImports(src) {
	const found = new Set();
	const importRe = /^[ \t]*import\s+(type\s+)?[^\n]*?from\s+['"]@poodle64\/ui\/([a-z0-9-]+)['"]/gm;
	let m;
	while ((m = importRe.exec(src))) {
		if (m[1]) continue;
		const component = m[2];
		if (shipped.has(component) && !PRIMITIVES.has(component)) found.add(component);
	}
	return found;
}

/**
 * A page's own presentation frequently lives one level down, not in the route
 * file itself. Only direct children (`$components`/`$lib` imports resolving to
 * a `.svelte` file) are followed; a grandchild is out of scope, matching "one
 * level deep" in the brief contract.
 */
function childComponentFiles(pageSrc) {
	const out = [];
	const importRe = /from\s+['"](\$components\/[^'"]+|\$lib\/[^'"]+)['"]/g;
	let m;
	while ((m = importRe.exec(pageSrc))) {
		const spec = m[1];
		if (!spec.endsWith('.svelte')) continue;
		const relPath = spec.startsWith('$components/')
			? spec.replace('$components/', 'lib/components/')
			: spec.replace('$lib/', 'lib/');
		const full = path.join(SRC, relPath);
		if (existsSync(full)) out.push(full);
	}
	return out;
}

const staleBriefs = [];

for (const surfaceFile of surfaceFiles) {
	const md = readFileSync(path.join(SURFACES_DIR, surfaceFile), 'utf8');
	const routeMatch = md.match(/^route:\s*(\S+)/m);
	if (!routeMatch) continue;
	const route = routeMatch[1];
	const pageFile = routeToPageFile.get(route);

	if (!pageFile) {
		staleBriefs.push(`docs/product/surfaces/${surfaceFile} (route: ${route})`);
		continue;
	}

	const declared = declaredVocabulary(briefSection(md, 'The decision'));
	const pageSrc = readFileSync(pageFile, 'utf8');
	const sources = [pageSrc, ...childComponentFiles(pageSrc).map((f) => readFileSync(f, 'utf8'))];

	const composed = new Set();
	for (const src of sources) {
		for (const c of composedImports(src)) composed.add(c);
	}

	for (const c of [...composed].sort()) {
		if (declared.has(c)) continue;
		findings.push({
			rule: 'surface-brief-divergence',
			file: rel(pageFile),
			// Carried as its own field, not just baked into `detail`, because the
			// baseline key needs it too — see the key() comment below.
			component: kebabToPascal(c),
			detail: `composes ${kebabToPascal(c)}, which docs/product/surfaces/${surfaceFile} does not name; update the brief first if the presentation decision has changed`
		});
	}
}

// ---------------------------------------------------------------------------
// Informational: shipped components this app never imports.
//
// NOT a failure — plenty are legitimately unneeded. It is printed so the list
// can be read the way an audit reads it: for each one, "do we hand-roll that?"
// ---------------------------------------------------------------------------
const allSource = files
	.filter((f) => f.endsWith('.svelte') || f.endsWith('.ts'))
	.map((f) => readFileSync(f, 'utf8'))
	.join('\n');
const unused = [...shipped]
	.filter((c) => !new RegExp(`@poodle64/ui/${c}\\b`).test(allSource))
	.sort();

// ---------------------------------------------------------------------------
// Baseline: gate on NEW drift, not on the backlog.
//
// A gate that fails on the day it lands gets disabled within a week, and then
// catches nothing forever. Grandfathering what already exists and failing only
// on what a change ADDS is what makes it survivable. A freshly stamped app
// ships this baseline EMPTY: the scaffold composes what the package ships, so
// there is nothing to grandfather. It is in copier's `_skip_if_exists`, so
// `copier update` never wipes the debt an app has since banked.
//
// The baseline is a debt register, not an amnesty: `--baseline` rewrites it, so
// shrinking it is a visible diff and growing it needs a deliberate act.
// ---------------------------------------------------------------------------
//
// The key is `${rule}:${file}` — deliberately NOT a line number or a selector,
// either of which churns on unrelated edits and re-flags a finding that never
// changed, which is how a baseline gate loses trust and gets bypassed.
//
// The granularity is deliberately NOT uniform across rules; do not "tidy" it
// back. `vendored-copy` and `hand-rolled-page-title` are one-finding-per-file
// by construction, so `${rule}:${file}` uniquely identifies each. But one page
// can compose several undeclared components at once, so keying
// `surface-brief-divergence` the same way collapses them into one entry:
// banking any one silently banks the rest, and a FURTHER divergence on an
// already-flagged page produces no fresh finding at all. That is a fail-open —
// the gate goes quiet exactly when a new divergence lands. Its key therefore
// carries the component name.
const key = (f) =>
	f.rule === 'surface-brief-divergence'
		? `${f.rule}:${f.file}:${f.component}`
		: `${f.rule}:${f.file}`;

if (process.argv.includes('--baseline')) {
	writeFileSync(BASELINE, JSON.stringify([...new Set(findings.map(key))].sort(), null, 2) + '\n');
	console.log(
		`Baseline written: ${findings.length} known finding(s) in ${path.relative(FRONTEND_ROOT, BASELINE)}`
	);
	process.exit(0);
}

const known = new Set(existsSync(BASELINE) ? JSON.parse(readFileSync(BASELINE, 'utf8')) : []);
const fresh = findings.filter((f) => !known.has(key(f)));
const fixed = [...known].filter((k) => !findings.some((f) => key(f) === k));

if (process.argv.includes('--json')) {
	console.log(
		JSON.stringify(
			{
				fresh,
				grandfathered: findings.length - fresh.length,
				fixed,
				unusedShippedComponents: unused,
				staleBriefs
			},
			null,
			2
		)
	);
	process.exit(fresh.length ? 1 : 0);
}

for (const f of fresh) console.error(`${f.file}\n  [${f.rule}] ${f.detail}`);
if (fresh.length) {
	console.error(`\n${fresh.length} NEW design-system drift finding(s).`);
	console.error('Compose what the package ships; a local copy cannot receive an upstream fix.');
} else {
	console.log(`No new design-system drift. (${known.size} known, grandfathered.)`);
}
if (fixed.length) {
	console.log(`\n${fixed.length} baseline finding(s) fixed — rerun with --baseline to bank it:`);
	for (const k of fixed) console.log(`  ${k}`);
}
if (unused.length) {
	console.log(`\nFYI — ${unused.length} shipped components this app never imports:`);
	console.log(`  ${unused.join(', ')}`);
	console.log('  Worth a glance: is any of them something a page here hand-rolls?');
}
if (staleBriefs.length) {
	console.log(`\nFYI — ${staleBriefs.length} surface brief(s) whose route no longer exists:`);
	for (const s of staleBriefs) console.log(`  ${s}`);
	console.log('  A brief for a route that has gone is a decision nobody is reading any more.');
}

process.exit(fresh.length ? 1 : 0);
