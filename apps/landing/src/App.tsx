import { useState } from "react"
import type { CSSProperties } from "react"
import type { LucideIcon } from "lucide-react"
import {
  ArrowRight,
  Braces,
  Check,
  CircleGauge,
  Copy,
  FileSearch,
  GitBranch,
  Github,
  Layers3,
  Menu,
  Network,
  ShieldCheck,
  Sparkles,
  Terminal,
  X,
  Zap,
} from "lucide-react"

import reposcoutArtwork from "../../web/src/assets/reposcout.png"
import reposcoutIcon from "./assets/reposcout-fox.png"
import { GraphShowcase } from "./GraphShowcase"
import { useSectionParallax } from "./useSectionParallax"

const githubUrl = "https://github.com/gordon1210/reposcout"
const releasesUrl = `${githubUrl}/releases/latest`
const docsUrl = `${githubUrl}/blob/main/docs/README.md`
const cliExamplesUrl = `${githubUrl}/blob/main/docs/cli-reference.md#common-examples`
const securityUrl = `${githubUrl}/security/policy`
const installCommand = "curl -fsSL https://getreposcout.vercel.app/install.sh | sh"

type Signal = {
  icon: LucideIcon
  label: string
  title: string
  copy: string
  className: string
}

const signals: Signal[] = [
  {
    icon: CircleGauge,
    label: "Context fit",
    title: "Know what fits before you load it.",
    copy: "Hard token budgets become an explainable reading order with compact, body-free symbol outlines.",
    className: "signal-card--context",
  },
  {
    icon: ShieldCheck,
    label: "Risk map",
    title: "Start where the code can hurt.",
    copy: "Complexity, churn, size, and matching-test signals combine into a ranked, explainable reading list.",
    className: "signal-card--risk",
  },
  {
    icon: Layers3,
    label: "Clone detection",
    title: "See duplication, not noise.",
    copy: "Exact and Type-2 matches are line-filtered, similarity-scored, and ranked by removable code.",
    className: "signal-card--duplicates",
  },
  {
    icon: Network,
    label: "Change impact",
    title: "Trace the blast radius.",
    copy: "Scope to a diff, then map unchanged importers that may be affected by the files you touched.",
    className: "signal-card--impact",
  },
]

const workflow = [
  {
    number: "01",
    title: "Scout",
    copy: "Run one local command against a repository, subdirectory, changeset, or file.",
  },
  {
    number: "02",
    title: "Decide",
    copy: "Use the compact assessment to set context, reading order, and task boundaries.",
  },
  {
    number: "03",
    title: "Act",
    copy: "Move into implementation with risk, tests, duplicates, and impact already mapped.",
  },
]

const capabilities = [
  "Token & context budgeting",
  "Structural context plans",
  "Per-function complexity",
  "Exact & near duplication",
  "Risk-ranked files",
  "Test-presence signals",
  "Git churn hotspots",
  "Architecture graph & impact",
  "Changed-line review",
]

function Brand() {
  return (
    <a className="brand" href="#top" aria-label="RepoScout home">
      <span className="brand__mark" aria-hidden="true">
        <img src={reposcoutIcon} alt="" />
      </span>
      <span className="brand__name">
        repo<span>scout</span>
      </span>
    </a>
  )
}

function CopyButton({
  copied,
  onCopy,
  compact = false,
}: {
  copied: boolean
  onCopy: () => void
  compact?: boolean
}) {
  return (
    <button
      className={compact ? "copy-button copy-button--compact" : "copy-button"}
      type="button"
      onClick={onCopy}
      aria-label="Copy install command"
    >
      {copied ? <Check size={16} /> : <Copy size={16} />}
      <span>{copied ? "Copied" : compact ? "Copy" : "Copy command"}</span>
    </button>
  )
}

export function App() {
  const [copied, setCopied] = useState(false)
  const [mobileOpen, setMobileOpen] = useState(false)
  useSectionParallax()

  const copyInstall = async () => {
    try {
      await navigator.clipboard.writeText(installCommand)
      setCopied(true)
      window.setTimeout(() => setCopied(false), 1800)
    } catch {
      setCopied(false)
    }
  }

  const closeMobileNav = () => setMobileOpen(false)

  return (
    <div className="page-shell">
      <a className="skip-link" href="#main">
        Skip to content
      </a>

      <header className="site-header">
        <div className="header-inner">
          <Brand />

          <nav className="desktop-nav" aria-label="Primary navigation">
            <a href="#workflow">Agent workflow</a>
            <a href="#signals">Signals</a>
            <a href="#graph">Graph</a>
            <a href="#open-source">Open source</a>
          </nav>

          <a className="header-github" href={githubUrl} target="_blank" rel="noreferrer">
            <Github size={17} />
            <span>View on GitHub</span>
            <ArrowRight size={15} />
          </a>

          <button
            className="mobile-toggle"
            type="button"
            aria-label={mobileOpen ? "Close navigation" : "Open navigation"}
            aria-expanded={mobileOpen}
            aria-controls="mobile-navigation"
            onClick={() => setMobileOpen((open) => !open)}
          >
            {mobileOpen ? <X size={21} /> : <Menu size={21} />}
          </button>
        </div>

        <nav
          id="mobile-navigation"
          className={mobileOpen ? "mobile-nav mobile-nav--open" : "mobile-nav"}
          aria-label="Mobile navigation"
        >
          <a href="#workflow" onClick={closeMobileNav}>
            Agent workflow
          </a>
          <a href="#signals" onClick={closeMobileNav}>
            Signals
          </a>
          <a href="#graph" onClick={closeMobileNav}>
            Graph
          </a>
          <a href="#open-source" onClick={closeMobileNav}>
            Open source
          </a>
          <a href={githubUrl} target="_blank" rel="noreferrer">
            GitHub <ArrowRight size={15} />
          </a>
        </nav>
      </header>

      <main id="main">
        <section className="hero" id="top" data-parallax-section>
          <div className="hero-grid" aria-hidden="true" />
          <div className="hero-glow hero-glow--cyan" aria-hidden="true" />
          <div className="hero-glow hero-glow--lime" aria-hidden="true" />

          <div className="hero-inner">
            <div className="hero-copy">
              <div className="hero-eyebrow">
                <span className="eyebrow-pulse" />
                Open-source repo intelligence
              </div>

              <h1>
                Know the repo
                <span className="hero-title-accent">before your agent</span>
                reads it.
              </h1>

              <p className="hero-lede">
                RepoScout turns an unfamiliar codebase into a compact, trustworthy map of
                context size, complexity, duplication, risk, and change impact—in seconds.
              </p>

              <div className="hero-actions">
                <a className="button button--primary" href="#workflow">
                  See the agent workflow
                  <ArrowRight size={18} />
                </a>
                <a className="button button--ghost" href={githubUrl} target="_blank" rel="noreferrer">
                  <Github size={18} />
                  GitHub
                </a>
              </div>

              <div className="install-command">
                <Terminal size={16} aria-hidden="true" />
                <code>{installCommand}</code>
                <CopyButton copied={copied} onCopy={() => void copyInstall()} compact />
              </div>

              <div className="hero-proof" aria-label="Product qualities">
                <span>
                  <Check size={14} /> Runs locally
                </span>
                <span>
                  <Check size={14} /> Rust-powered
                </span>
                <span>
                  <Check size={14} /> Agent-ready JSON
                </span>
              </div>
            </div>

            <div className="hero-visual">
              <div className="orbit orbit--outer" aria-hidden="true" />
              <div className="orbit orbit--inner" aria-hidden="true" />
              <div className="hero-art-halo" aria-hidden="true" />
              <img
                className="hero-art"
                src={reposcoutArtwork}
                alt="RepoScout fox scout looking through binoculars"
              />

              <div className="floating-chip floating-chip--top">
                <span className="chip-icon chip-icon--cyan">
                  <Braces size={16} />
                </span>
                <span>
                  <small>OUTPUT</small>
                  Structured JSON
                </span>
              </div>

              <div className="floating-chip floating-chip--bottom">
                <span className="chip-icon chip-icon--lime">
                  <Zap size={16} />
                </span>
                <span>
                  <small>SCOPE</small>
                  Repo → single file
                </span>
              </div>

              <div className="hero-coordinate hero-coordinate--top" aria-hidden="true">
                52° / SIGNAL
              </div>
              <div className="hero-coordinate hero-coordinate--bottom" aria-hidden="true">
                LOCAL / 0.1.0
              </div>
            </div>
          </div>

          <div className="signal-ticker" aria-label="RepoScout analysis signals">
            <span>Tokens</span>
            <i />
            <span>Complexity</span>
            <i />
            <span>Duplication</span>
            <i />
            <span>Risk</span>
            <i />
            <span>Churn</span>
            <i />
            <span>Change impact</span>
          </div>
        </section>

        <section className="thesis section-pad">
          <div className="section-inner thesis-grid">
            <div className="section-kicker">Why scout first?</div>
            <div className="thesis-copy">
              <h2>
                Repositories are vast.
                <br />
                <em>Engineer the context.</em>
              </h2>
              <p>
                “Read the repo” is not a plan. RepoScout gives an agent enough evidence to choose
                what matters, what can be skipped, and where a change is most likely to ripple.
              </p>
            </div>
            <div className="before-after">
              <div>
                <span>Without a scout</span>
                <strong>Read everything</strong>
                <small>Spend context before knowing relevance.</small>
              </div>
              <div>
                <span>With RepoScout</span>
                <strong>Read with intent</strong>
                <small>Start with the highest-signal files and seams.</small>
              </div>
            </div>
          </div>
        </section>

        <section className="workflow section-pad" id="workflow">
          <div className="section-inner">
            <div className="section-heading">
              <div>
                <div className="section-kicker section-kicker--light">Agent workflow</div>
                <h2>
                  Give your agent a map,
                  <br />
                  <em>not a haystack.</em>
                </h2>
              </div>
              <p>
                One compact scan becomes the first piece of context—before file reads, plans,
                delegation, or edits.
              </p>
            </div>

            <div className="agent-demo">
              <div className="prompt-panel">
                <div className="panel-topline">
                  <span>AGENT BRIEF</span>
                  <span className="live-label">
                    <i /> READY
                  </span>
                </div>

                <div className="prompt-bubble">
                  <span className="prompt-avatar">YOU</span>
                  <p>
                    Scout this repository before making changes. Tell me whether it fits context,
                    what to skip, and where the highest risk lives.
                  </p>
                </div>

                <div className="agent-action">
                  <span className="agent-avatar">
                    <Sparkles size={17} />
                  </span>
                  <div>
                    <small>AGENT RUNS</small>
                    <code>
                      <span>$</span> reposcout -f json --summary .
                    </code>
                  </div>
                </div>

                <div className="prompt-note">
                  <FileSearch size={17} />
                  <span>
                    No source leaves the machine.
                    <small>Discovery respects .gitignore and .reposcoutignore.</small>
                  </span>
                </div>
              </div>

              <div className="report-panel">
                <div className="terminal-bar">
                  <div className="terminal-dots" aria-hidden="true">
                    <i />
                    <i />
                    <i />
                  </div>
                  <span>reposcout · summary.json</span>
                  <span>1.8 KB</span>
                </div>

                <div className="json-report" aria-label="Example RepoScout JSON summary">
                  <div>
                    <span className="json-muted">{"{"}</span>
                  </div>
                  <div className="json-indent">
                    <span className="json-key">"summary"</span>
                    <span className="json-muted">: {"{"}</span>
                  </div>
                  <div className="json-indent json-indent--two">
                    <span className="json-key">"tokens"</span>
                    <span className="json-muted">: </span>
                    <span className="json-number">184720</span>
                    <span className="json-muted">,</span>
                  </div>
                  <div className="json-indent json-indent--two">
                    <span className="json-key">"assessment"</span>
                    <span className="json-muted">: {"{"}</span>
                  </div>
                  <div className="json-indent json-indent--three json-highlight">
                    <span className="json-key">"fits_context"</span>
                    <span className="json-muted">: </span>
                    <span className="json-bool">true</span>
                    <span className="json-muted">,</span>
                  </div>
                  <div className="json-indent json-indent--three">
                    <span className="json-key">"cleanup_worth"</span>
                    <span className="json-muted">: </span>
                    <span className="json-string">"medium"</span>
                  </div>
                  <div className="json-indent json-indent--two">
                    <span className="json-muted">{"}"},</span>
                  </div>
                  <div className="json-indent json-indent--two">
                    <span className="json-key">"top_risks"</span>
                    <span className="json-muted">: [</span>
                  </div>
                  <div className="json-indent json-indent--three">
                    <span className="json-muted">{"{"} </span>
                    <span className="json-key">"path"</span>
                    <span className="json-muted">: </span>
                    <span className="json-string">"src/scan.rs"</span>
                    <span className="json-muted">, </span>
                    <span className="json-key">"score"</span>
                    <span className="json-muted">: </span>
                    <span className="json-number">0.82</span>
                    <span className="json-muted"> {"}"}</span>
                  </div>
                  <div className="json-indent json-indent--two">
                    <span className="json-muted">]</span>
                  </div>
                  <div className="json-indent">
                    <span className="json-muted">{"}"}</span>
                  </div>
                  <div>
                    <span className="json-muted">{"}"}</span>
                  </div>
                </div>

                <div className="verdict-row">
                  <span className="verdict-icon">
                    <Check size={18} />
                  </span>
                  <div>
                    <small>AGENT VERDICT</small>
                    <strong>Fits context. Read 4 risk-ranked files first.</strong>
                  </div>
                </div>
              </div>
            </div>

            <div className="workflow-steps">
              {workflow.map((step) => (
                <article key={step.number}>
                  <span>{step.number}</span>
                  <div>
                    <h3>{step.title}</h3>
                    <p>{step.copy}</p>
                  </div>
                </article>
              ))}
            </div>
          </div>
        </section>

        <section className="signals section-pad" id="signals" data-parallax-section>
          <div className="section-inner">
            <div className="section-heading section-heading--ink">
              <div>
                <div className="section-kicker">The signal layer</div>
                <h2>
                  A repo, reduced to
                  <br />
                  <em>the decisions that matter.</em>
                </h2>
              </div>
              <p>
                Every metric is designed to change what an agent reads, plans, or verifies next.
              </p>
            </div>

            <div className="signal-grid">
              {signals.map((signal, index) => {
                const Icon = signal.icon

                return (
                  <article className={"signal-card " + signal.className} key={signal.label}>
                    <div className="signal-card__top">
                      <span className="signal-card__icon">
                        <Icon size={20} />
                      </span>
                      <span className="signal-card__index">0{index + 1}</span>
                    </div>
                    <div className="signal-card__copy">
                      <small>{signal.label}</small>
                      <h3>{signal.title}</h3>
                      <p>{signal.copy}</p>
                    </div>

                    {index === 0 && (
                      <div className="context-viz" aria-hidden="true">
                        <div className="context-viz__labels">
                          <span>184.7K used</span>
                          <span>200K context</span>
                        </div>
                        <div className="context-viz__track">
                          <i />
                        </div>
                        <strong>92%</strong>
                      </div>
                    )}

                    {index === 1 && (
                      <div className="risk-viz" aria-hidden="true">
                        <span style={{ "--risk": "82%" } as CSSProperties}>
                          <i>scan.rs</i>
                          <b>0.82</b>
                        </span>
                        <span style={{ "--risk": "61%" } as CSSProperties}>
                          <i>graph.rs</i>
                          <b>0.61</b>
                        </span>
                        <span style={{ "--risk": "43%" } as CSSProperties}>
                          <i>review.rs</i>
                          <b>0.43</b>
                        </span>
                      </div>
                    )}

                    {index === 2 && (
                      <div className="duplicate-viz" aria-hidden="true">
                        <div>
                          <span>EXACT</span>
                          <strong>12</strong>
                          <small>groups</small>
                        </div>
                        <div>
                          <span>TYPE-2</span>
                          <strong>08</strong>
                          <small>groups</small>
                        </div>
                        <div className="duplicate-viz__ring">
                          <b>7.4%</b>
                          <small>covered</small>
                        </div>
                      </div>
                    )}

                    {index === 3 && (
                      <div className="impact-viz" aria-hidden="true">
                        <div className="impact-node impact-node--changed">changed.ts</div>
                        <svg
                          className="impact-edge impact-edge--up"
                          viewBox="0 0 100 100"
                          preserveAspectRatio="none"
                        >
                          <defs>
                            <marker
                              id="impact-arrow-up"
                              markerWidth="5"
                              markerHeight="5"
                              refX="4.5"
                              refY="2.5"
                              orient="auto"
                            >
                              <path d="M0,0 L5,2.5 L0,5 Z" />
                            </marker>
                          </defs>
                          <line
                            x1="0"
                            y1="72"
                            x2="100"
                            y2="28"
                            markerEnd="url(#impact-arrow-up)"
                          />
                        </svg>
                        <div className="impact-node impact-node--direct">direct.ts</div>
                        <svg
                          className="impact-edge impact-edge--down"
                          viewBox="0 0 100 100"
                          preserveAspectRatio="none"
                        >
                          <defs>
                            <marker
                              id="impact-arrow-down"
                              markerWidth="5"
                              markerHeight="5"
                              refX="4.5"
                              refY="2.5"
                              orient="auto"
                            >
                              <path d="M0,0 L5,2.5 L0,5 Z" />
                            </marker>
                          </defs>
                          <line
                            x1="0"
                            y1="28"
                            x2="100"
                            y2="72"
                            markerEnd="url(#impact-arrow-down)"
                          />
                        </svg>
                        <div className="impact-node impact-node--transitive">route.ts</div>
                      </div>
                    )}
                  </article>
                )
              })}
            </div>
          </div>
        </section>

        <GraphShowcase />

        <section className="change-section section-pad" data-parallax-section>
          <div className="section-inner change-grid">
            <div className="change-copy">
              <div className="section-kicker">Change-aware by design</div>
              <h2>
                Scout the whole forest.
                <br />
                Or just <em>the fresh tracks.</em>
              </h2>
              <p>
                Narrow every signal to staged work, your working tree, or a branch diff. Then
                plan the changed files, tests, and dependents worth reading; review changed lines;
                and gate regressions with the same contract.
              </p>
              <a href={cliExamplesUrl} target="_blank" rel="noreferrer">
                Explore the CLI examples <ArrowRight size={17} />
              </a>
            </div>

            <div className="command-stack" aria-label="RepoScout change-aware command examples">
              <div>
                <span>01</span>
                <code>reposcout --working --context --impact .</code>
                <GitBranch size={18} />
              </div>
              <div>
                <span>02</span>
                <code>reposcout --since main --review=deep .</code>
                <FileSearch size={18} />
              </div>
              <div>
                <span>03</span>
                <code>reposcout --baseline baseline.json --fail-on-regression .</code>
                <ShieldCheck size={18} />
              </div>
            </div>
          </div>
        </section>

        <section className="capabilities section-pad" id="open-source">
          <div className="section-inner capabilities-grid">
            <div className="capability-intro">
              <div className="section-kicker">One binary, broad visibility</div>
              <h2>
                Built for agents.
                <br />
                Useful to <em>humans.</em>
              </h2>
              <p>
                Human-readable terminal and Markdown reports share the same stable model as JSON,
                NDJSON, and SARIF. No second source of truth.
              </p>

              <div className="format-row">
                <span>table</span>
                <span>json</span>
                <span>markdown</span>
                <span>sarif</span>
                <span>ndjson</span>
                <span>dot</span>
                <span>mermaid</span>
              </div>
            </div>

            <div className="capability-list">
              {capabilities.map((capability, index) => (
                <div key={capability}>
                  <span>{String(index + 1).padStart(2, "0")}</span>
                  <strong>{capability}</strong>
                  <Check size={17} />
                </div>
              ))}
            </div>
          </div>

          <div className="section-inner language-strip">
            <span className="language-strip__label">AST-FIRST</span>
            <div>
              <strong>Rust</strong>
              <i />
              <strong>Python</strong>
              <i />
              <strong>TypeScript</strong>
              <i />
              <strong>JavaScript</strong>
              <i />
              <strong>Go</strong>
              <i />
              <strong>PHP</strong>
            </div>
            <span className="language-strip__note">6 first-class · 31 recognized formats</span>
          </div>
        </section>

        <section className="final-cta section-pad" data-parallax-section>
          <div className="final-grid" aria-hidden="true" />
          <div className="final-orbit-frame" aria-hidden="true">
            <div className="final-orbit" />
          </div>
          <div className="section-inner final-cta__inner">
            <div className="final-cta__icon">
              <img src={reposcoutIcon} alt="" />
            </div>
            <div className="section-kicker section-kicker--light">Your repo has a signal</div>
            <h2>
              Find it before
              <br />
              <em>your agent gets lost.</em>
            </h2>
            <p>Fast to run. Small enough for context. Specific enough to act on.</p>

            <div className="final-install">
              <Terminal size={18} aria-hidden="true" />
              <code>{installCommand}</code>
              <CopyButton copied={copied} onCopy={() => void copyInstall()} />
            </div>

            <a className="final-github" href={githubUrl} target="_blank" rel="noreferrer">
              <Github size={18} />
              Browse the source on GitHub
              <ArrowRight size={17} />
            </a>
          </div>
        </section>
      </main>

      <footer className="site-footer">
        <div className="section-inner footer-inner">
          <Brand />
          <p>Fast repository intelligence for agents and humans.</p>
          <div>
            <a href={githubUrl} target="_blank" rel="noreferrer">
              GitHub
            </a>
            <a href={releasesUrl} target="_blank" rel="noreferrer">
              Releases
            </a>
            <a href={docsUrl} target="_blank" rel="noreferrer">
              Docs
            </a>
            <a href={securityUrl} target="_blank" rel="noreferrer">
              Security
            </a>
          </div>
        </div>
      </footer>
    </div>
  )
}
