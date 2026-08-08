import { execFileSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);

const SECTIONS = [{ dir: "docs/cli", prefix: "CLI", label: "CLI reference" }];

const GUIDES = [
  { src: "docs/guide.md", slug: "Guide", title: "Guide" },
  {
    src: "docs/claude-snippet.md",
    slug: "Claude-Snippet",
    title: "Claude Snippet",
  },
  { src: "CLAUDE.md", slug: "Operating-Manual", title: "Operating Manual" },
];

const SMALL = new Set([
  "a",
  "an",
  "the",
  "of",
  "to",
  "and",
  "in",
  "on",
  "for",
  "with",
  "vs",
]);

const ACRONYMS = new Map([
  ["cli", "CLI"],
  ["json", "JSON"],
  ["api", "API"],
  ["url", "URL"],
  ["tty", "TTY"],
]);

function cap(word) {
  if (ACRONYMS.has(word)) return ACRONYMS.get(word);
  return word ? word[0].toUpperCase() + word.slice(1) : word;
}

function slugSuffix(name) {
  return name
    .split("-")
    .map((seg, i) => (i > 0 && SMALL.has(seg) ? seg : cap(seg)))
    .join("-");
}

function displayTitle(name) {
  return slugSuffix(name).replaceAll("-", " ");
}

function ownerRepo() {
  try {
    const url = execFileSync("git", ["remote", "get-url", "origin"], {
      cwd: repoRoot,
    })
      .toString()
      .trim();
    const match = url.match(/github\.com[:/]+([^/]+)\/(.+?)(?:\.git)?$/);
    if (match) return { owner: match[1], repo: match[2] };
  } catch {}
  return { owner: "pulkitxm", repo: "ac" };
}

function collect() {
  const docs = [];
  for (const section of SECTIONS) {
    const abs = path.join(repoRoot, section.dir);
    if (!existsSync(abs)) continue;
    const files = readdirSync(abs)
      .filter((f) => f.endsWith(".md"))
      .sort();
    for (const file of files) {
      const name = file.replace(/\.md$/, "");
      const src = `${section.dir}/${file}`;
      if (file.toLowerCase() === "readme.md") {
        docs.push({
          src,
          slug: section.prefix,
          title: "Overview",
          section: section.label,
          isIndex: true,
        });
      } else {
        docs.push({
          src,
          slug: `${section.prefix}-${slugSuffix(name)}`,
          title: displayTitle(name),
          section: section.label,
          isIndex: false,
        });
      }
    }
  }
  for (const guide of GUIDES) {
    if (!existsSync(path.join(repoRoot, guide.src))) continue;
    docs.push({
      src: guide.src,
      slug: guide.slug,
      title: guide.title,
      section: "Guides",
      isIndex: false,
    });
  }
  return docs;
}

const { owner, repo } = ownerRepo();
const blobBase = `https://github.com/${owner}/${repo}/blob/main`;
const treeBase = `https://github.com/${owner}/${repo}/tree/main`;
const wikiDisplay = `https://github.com/${owner}/${repo}.wiki.git`;

const docs = collect();
const slugMap = new Map(docs.map((d) => [d.src, d.slug]));
const dirMap = new Map(SECTIONS.map((s) => [s.dir, s.prefix]));

function mapTarget(target, fileDir) {
  const parts = target.split(/\s+/);
  const url = parts[0];
  const titleRest = parts.length > 1 ? ` ${parts.slice(1).join(" ")}` : "";
  if (url === "" || /^(https?:|mailto:|#)/i.test(url)) return null;
  const hash = url.indexOf("#");
  const pathPart = hash >= 0 ? url.slice(0, hash) : url;
  const anchor = hash >= 0 ? url.slice(hash) : "";
  if (pathPart === "") return null;
  const resolved = path.posix
    .normalize(path.posix.join(fileDir, pathPart))
    .replace(/\/$/, "");
  if (dirMap.has(resolved))
    return `${dirMap.get(resolved)}${anchor}${titleRest}`;
  if (slugMap.has(resolved))
    return `${slugMap.get(resolved)}${anchor}${titleRest}`;
  if (existsSync(path.join(repoRoot, resolved)))
    return `${blobBase}/${resolved}${anchor}${titleRest}`;
  return null;
}

function rewriteLinks(content, fileDir) {
  let inFence = false;
  return content
    .split("\n")
    .map((line) => {
      const trimmed = line.trimStart();
      if (trimmed.startsWith("```") || trimmed.startsWith("~~~")) {
        inFence = !inFence;
        return line;
      }
      if (inFence) return line;
      return line.replace(/\]\(([^)]+)\)/g, (whole, target) => {
        const mapped = mapTarget(target, fileDir);
        return mapped === null ? whole : `](${mapped})`;
      });
    })
    .join("\n");
}

function sectionDocs(label) {
  const inSection = docs.filter((d) => d.section === label);
  const index = inSection.filter((d) => d.isIndex);
  const rest = inSection
    .filter((d) => !d.isIndex)
    .sort((a, b) => a.title.localeCompare(b.title));
  return [...index, ...rest];
}

function endWithNewline(text) {
  return `${text.replace(/\s*$/, "")}\n`;
}

function buildHome() {
  const lines = [
    "Reference documentation for **ac**, the CLI for Apple `container`, auto-generated from the `docs/` directory of the main repository. Edit the docs in the repo; these pages are overwritten on every push to `main`.",
    "",
  ];
  for (const section of SECTIONS) {
    lines.push(`## ${section.label}`, "");
    for (const doc of sectionDocs(section.label))
      lines.push(`- [${doc.title}](${doc.slug})`);
    lines.push("");
  }
  const guides = docs.filter((d) => d.section === "Guides");
  if (guides.length) {
    lines.push("## Guides", "");
    for (const doc of guides) lines.push(`- [${doc.title}](${doc.slug})`);
    lines.push("");
  }
  return endWithNewline(lines.join("\n"));
}

function buildSidebar() {
  const lines = ["- [Home](Home)", ""];
  for (const section of SECTIONS) {
    lines.push(`**${section.label}**`, "");
    for (const doc of sectionDocs(section.label))
      lines.push(`- [${doc.title}](${doc.slug})`);
    lines.push("");
  }
  const guides = docs.filter((d) => d.section === "Guides");
  if (guides.length) {
    lines.push("**Guides**", "");
    for (const doc of guides) lines.push(`- [${doc.title}](${doc.slug})`);
    lines.push("");
  }
  return endWithNewline(lines.join("\n"));
}

function buildFooter() {
  return endWithNewline(
    `_Auto-generated from [\`docs/\`](${treeBase}/docs); edit the docs in the repo, not the wiki._`,
  );
}

function buildPages() {
  const pages = new Map();
  for (const doc of docs) {
    const raw = readFileSync(path.join(repoRoot, doc.src), "utf8");
    const body = rewriteLinks(raw, path.posix.dirname(doc.src));
    pages.set(`${doc.slug}.md`, endWithNewline(body));
  }
  pages.set("Home.md", buildHome());
  pages.set("_Sidebar.md", buildSidebar());
  pages.set("_Footer.md", buildFooter());
  return pages;
}

function clearMarkdown(dir) {
  for (const entry of readdirSync(dir)) {
    if (entry === ".git") continue;
    if (entry.endsWith(".md")) rmSync(path.join(dir, entry), { force: true });
  }
}

function writePages(dir, pages) {
  mkdirSync(dir, { recursive: true });
  clearMarkdown(dir);
  for (const [name, content] of pages)
    writeFileSync(path.join(dir, name), content);
}

function configIdentity(dir) {
  const name = process.env.GIT_AUTHOR_NAME || "wiki-sync";
  const email =
    process.env.GIT_AUTHOR_EMAIL || "wiki-sync@users.noreply.github.com";
  execFileSync("git", ["config", "user.name", name], { cwd: dir });
  execFileSync("git", ["config", "user.email", email], { cwd: dir });
}

function pushPages(pages) {
  const token = process.env.GITHUB_TOKEN || process.env.GH_TOKEN || "";
  const auth = token ? `x-access-token:${token}@` : "";
  const wikiUrl = `https://${auth}github.com/${owner}/${repo}.wiki.git`;
  const dir = path.join(repoRoot, ".wiki-clone");
  rmSync(dir, { recursive: true, force: true });
  let cloned = false;
  try {
    execFileSync("git", ["clone", "--depth", "1", wikiUrl, dir], {
      stdio: "inherit",
    });
    cloned = true;
  } catch {
    console.log(`Wiki not initialized; bootstrapping ${wikiDisplay}`);
  }
  if (!cloned) {
    mkdirSync(dir, { recursive: true });
    execFileSync("git", ["init", "-b", "master"], {
      cwd: dir,
      stdio: "inherit",
    });
    execFileSync("git", ["remote", "add", "origin", wikiUrl], {
      cwd: dir,
      stdio: "inherit",
    });
  }
  configIdentity(dir);
  writePages(dir, pages);
  execFileSync("git", ["add", "-A"], { cwd: dir, stdio: "inherit" });
  const status = execFileSync("git", ["status", "--porcelain"], { cwd: dir })
    .toString()
    .trim();
  if (!status) {
    console.log("Wiki already up to date.");
    return;
  }
  execFileSync("git", ["commit", "-m", "docs: sync wiki from docs/"], {
    cwd: dir,
    stdio: "inherit",
  });
  try {
    execFileSync("git", ["push", "origin", "HEAD"], {
      cwd: dir,
      stdio: "inherit",
    });
  } catch {
    if (!cloned) {
      console.error(
        [
          "",
          `Could not push to ${wikiDisplay}.`,
          "",
          "The wiki has never been created, and GitHub only provisions the wiki",
          "git repository once its first page exists. Enabling the wiki in repo",
          "settings is not enough, and there is no API for it.",
          "",
          `Fix it once, by hand: open https://github.com/${owner}/${repo}/wiki,`,
          "click 'Create the first page', save anything at all, then re-run this",
          "workflow. Every page is overwritten on the next sync, so what you type",
          "does not matter.",
          "",
        ].join("\n"),
      );
      process.exit(1);
    }
    console.error(`\nPush to ${wikiDisplay} failed.\n`);
    process.exit(1);
  }
  console.log(`Pushed ${pages.size} pages to ${wikiDisplay}`);
}

function main() {
  const args = process.argv.slice(2);
  const pages = buildPages();
  if (args.includes("--push")) {
    pushPages(pages);
    return;
  }
  const outIndex = args.indexOf("--out");
  const outDir =
    outIndex >= 0
      ? path.resolve(args[outIndex + 1])
      : path.join(repoRoot, ".wiki-build");
  writePages(outDir, pages);
  console.log(
    `Wrote ${pages.size} pages to ${path.relative(repoRoot, outDir) || outDir}`,
  );
}

main();
