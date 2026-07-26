import { useState } from "react"
import {
  ArrowDownToLine,
  ArrowUpFromLine,
  Braces,
  ChevronRight,
  FileCode2,
  Layers3,
  Network,
  Search,
  Terminal,
} from "lucide-react"

type GraphNode = {
  id: string
  file: string
  path: string
  kind: string
  language: string
  role: string
  summary: string
  relation: string
  fanIn: number
  fanOut: number
  tokens: string
  complexity: number
  position: string
  accent: "php" | "typescript" | "go"
  prominent?: boolean
}

const graphNodes: GraphNode[] = [
  {
    id: "http-client",
    file: "HttpClient.php",
    path: "src/Http/HttpClient.php",
    kind: "SOURCE",
    language: "PHP",
    role: "BASE CLASS · 8",
    summary: "Shared HTTP client behavior used across the application integration layer.",
    relation: "8 resolved types directly extend HttpClient.",
    fanIn: 27,
    fanOut: 2,
    tokens: "660",
    complexity: 5,
    position: "focus",
    accent: "php",
    prominent: true,
  },
  {
    id: "contract",
    file: "ClientInterface.php",
    path: "src/Contracts/ClientInterface.php",
    kind: "CONTRACT",
    language: "PHP",
    role: "IMPLEMENTS",
    summary: "The public transport contract implemented by the selected client hierarchy.",
    relation: "HttpClient implements this interface through an explicit declaration.",
    fanIn: 9,
    fanOut: 0,
    tokens: "214",
    complexity: 0,
    position: "contract",
    accent: "php",
  },
  {
    id: "symfony",
    file: "SymfonyClient.php",
    path: "src/Http/SymfonyClient.php",
    kind: "SOURCE",
    language: "PHP",
    role: "EXTENDS",
    summary: "Symfony transport adapter that inherits the shared HTTP behavior.",
    relation: "SymfonyClient directly extends HttpClient.",
    fanIn: 2,
    fanOut: 3,
    tokens: "488",
    complexity: 3,
    position: "symfony",
    accent: "php",
  },
  {
    id: "guzzle",
    file: "GuzzleClient.php",
    path: "src/Http/GuzzleClient.php",
    kind: "SOURCE",
    language: "PHP",
    role: "EXTENDS",
    summary: "Guzzle-backed transport adapter for production HTTP requests.",
    relation: "GuzzleClient directly extends HttpClient.",
    fanIn: 6,
    fanOut: 4,
    tokens: "731",
    complexity: 4,
    position: "guzzle",
    accent: "php",
  },
  {
    id: "checkout-page",
    file: "CheckoutPage.tsx",
    path: "web/src/routes/CheckoutPage.tsx",
    kind: "SOURCE",
    language: "TypeScript",
    role: "ROUTE",
    summary: "Checkout route that coordinates cart state and the typed API client.",
    relation: "CheckoutPage imports api-client through a configured package alias.",
    fanIn: 1,
    fanOut: 4,
    tokens: "845",
    complexity: 7,
    position: "checkout",
    accent: "typescript",
  },
  {
    id: "api-client",
    file: "api-client.ts",
    path: "web/src/lib/api-client.ts",
    kind: "SOURCE",
    language: "TypeScript",
    role: "SHARED DEPENDENCY",
    summary: "Typed transport shared by the storefront routes and data hooks.",
    relation: "11 TypeScript files directly import api-client.",
    fanIn: 11,
    fanOut: 2,
    tokens: "512",
    complexity: 3,
    position: "api-client",
    accent: "typescript",
  },
  {
    id: "go-handler",
    file: "handler.go",
    path: "services/payments/handler.go",
    kind: "SOURCE",
    language: "Go",
    role: "ENTRYPOINT",
    summary: "HTTP boundary for the payment service and its request lifecycle.",
    relation: "handler.go imports the local payments service package.",
    fanIn: 1,
    fanOut: 3,
    tokens: "604",
    complexity: 4,
    position: "go-handler",
    accent: "go",
  },
  {
    id: "go-service",
    file: "service.go",
    path: "services/payments/service.go",
    kind: "SOURCE",
    language: "Go",
    role: "PACKAGE HUB",
    summary: "Core payment orchestration shared by transport and worker entrypoints.",
    relation: "4 Go files import the payments service package.",
    fanIn: 4,
    fanOut: 2,
    tokens: "928",
    complexity: 8,
    position: "go-service",
    accent: "go",
  },
]

const graphEdges = [
  {
    id: "php-contract",
    source: "http-client",
    target: "contract",
    path: "M 285 280 C 230 245, 175 150, 105 115",
  },
  {
    id: "php-symfony",
    source: "symfony",
    target: "http-client",
    path: "M 360 100 C 350 165, 320 225, 285 280",
  },
  {
    id: "php-guzzle",
    source: "guzzle",
    target: "http-client",
    path: "M 105 445 C 160 405, 225 340, 285 280",
  },
  {
    id: "typescript-import",
    source: "checkout-page",
    target: "api-client",
    path: "M 665 120 C 720 135, 760 175, 805 205",
  },
  {
    id: "go-import",
    source: "go-handler",
    target: "go-service",
    path: "M 665 370 C 720 390, 760 430, 805 460",
  },
]

export function GraphShowcase() {
  const [selectedId, setSelectedId] = useState("http-client")
  const selected = graphNodes.find((node) => node.id === selectedId) ?? graphNodes[0]

  return (
    <section className="graph-feature section-pad" id="graph" data-parallax-section>
      <div className="section-inner">
        <div className="graph-feature__heading">
          <div>
            <div className="section-kicker section-kicker--light">One topology · two interfaces</div>
            <h2>
              Explore the system.
              <br />
              Or <em>query it.</em>
            </h2>
          </div>
          <div className="graph-feature__intro">
            <p>
              Navigate mixed-language architecture in the dashboard, then ask the same stable
              graph-focused questions from a terminal, script, or agent.
            </p>
            <div>
              <span>6 first-class languages</span>
              <span>Composer-aware PHP</span>
              <span>Mixed-language topology</span>
            </div>
          </div>
        </div>

        <div className="graph-preview">
          <div className="graph-preview__bar">
            <div className="graph-breadcrumbs" aria-label="Example graph breadcrumb">
              <strong>Project</strong>
              <ChevronRight size={13} />
              <span>app</span>
              <ChevronRight size={13} />
              <span>src</span>
              <ChevronRight size={13} />
              <span>Http</span>
            </div>
            <div className="graph-preview__modes" aria-hidden="true">
              <span className="graph-preview__mode graph-preview__mode--active">Architecture</span>
              <span className="graph-preview__mode">Relations</span>
            </div>
          </div>

          <div className="graph-preview__tools">
            <div>
              <Search size={14} />
              <span>src/Http/HttpClient.php</span>
            </div>
            <span className="graph-preview__locate">Locate</span>
            <span>Both directions</span>
            <span>2 hops</span>
          </div>

          <div className="graph-preview__status">
            <strong>Showing 8 of 42 files</strong>
            <span>5 visible relationships</span>
            <span>PHP · TypeScript · Go</span>
          </div>

          <div className="graph-preview__body">
            <div
              className="graph-canvas"
              aria-label="Interactive example of a mixed-language repository graph"
            >
              <div className="graph-canvas__grid" aria-hidden="true" />
              <div className="graph-boundary graph-boundary--php" aria-hidden="true">
                <span>backend · Composer / PHP</span>
              </div>
              <div className="graph-boundary graph-boundary--typescript" aria-hidden="true">
                <span>web · npm / TypeScript</span>
              </div>
              <div className="graph-boundary graph-boundary--go" aria-hidden="true">
                <span>payments · Go module</span>
              </div>

              <svg
                className="graph-edges"
                viewBox="0 0 900 560"
                preserveAspectRatio="none"
                aria-hidden="true"
              >
                <defs>
                  <marker
                    id="graph-preview-arrow"
                    markerWidth="6"
                    markerHeight="6"
                    refX="5.2"
                    refY="3"
                    orient="auto"
                  >
                    <path d="M0,0 L6,3 L0,6 Z" />
                  </marker>
                </defs>
                {graphEdges.map((edge) => (
                  <path
                    className={
                      selectedId === edge.source || selectedId === edge.target
                        ? `graph-edge graph-edge--${edge.id} graph-edge--active`
                        : `graph-edge graph-edge--${edge.id}`
                    }
                    d={edge.path}
                    key={edge.id}
                    markerEnd="url(#graph-preview-arrow)"
                  />
                ))}
              </svg>

              {graphNodes.map((node) => (
                <button
                  className={[
                    "graph-node",
                    `graph-node--${node.position}`,
                    `graph-node--language-${node.accent}`,
                    node.prominent ? "graph-node--prominent" : "",
                    selectedId === node.id ? "graph-node--selected" : "",
                  ]
                    .filter(Boolean)
                    .join(" ")}
                  type="button"
                  aria-pressed={selectedId === node.id}
                  onClick={() => setSelectedId(node.id)}
                  key={node.id}
                >
                  <span className="graph-node__topline">
                    <small>{node.kind}</small>
                    <i>{node.role}</i>
                  </span>
                  <strong>{node.file}</strong>
                  <span className="graph-node__path">{node.path}</span>
                  <span className="graph-node__facts">
                    {node.tokens} tok&nbsp;&nbsp; {node.fanIn} in · {node.fanOut} out
                  </span>
                </button>
              ))}

              <div className="graph-minimap" aria-hidden="true">
                <i className="graph-minimap__viewport" />
                <span className="graph-minimap__node graph-minimap__node--one" />
                <span className="graph-minimap__node graph-minimap__node--two" />
                <span className="graph-minimap__node graph-minimap__node--three" />
                <span className="graph-minimap__node graph-minimap__node--four" />
                <span className="graph-minimap__node graph-minimap__node--five" />
              </div>
            </div>

            <aside className="graph-inspector" aria-live="polite">
              <div className="graph-inspector__tabs" aria-hidden="true">
                <span>Info</span>
                <span>Relations</span>
              </div>

              <div className="graph-inspector__badges">
                <span>{selected.kind}</span>
                <span>{selected.language}</span>
              </div>
              <h3>{selected.file}</h3>
              <code>{selected.path}</code>
              <p>{selected.summary}</p>

              <div className="graph-inspector__actions" aria-hidden="true">
                <span>
                  <ArrowDownToLine size={14} /> Dependencies
                </span>
                <span>
                  <ArrowUpFromLine size={14} /> Blast radius
                </span>
              </div>

              <div className="graph-inspector__reach">
                <div>
                  <Network size={15} />
                  <span>STRUCTURAL REACH</span>
                </div>
                <strong>{selected.relation}</strong>
                <small>Resolved from explicit syntax and repository configuration.</small>
              </div>

              <div className="graph-inspector__metrics">
                <div>
                  <span>FAN IN / OUT</span>
                  <strong>
                    {selected.fanIn} / {selected.fanOut}
                  </strong>
                </div>
                <div>
                  <span>TOKENS</span>
                  <strong>{selected.tokens}</strong>
                </div>
                <div>
                  <span>CYCLOMATIC</span>
                  <strong>{selected.complexity}</strong>
                </div>
                <div>
                  <span>LANGUAGE</span>
                  <strong>{selected.language}</strong>
                </div>
              </div>
            </aside>
          </div>

          <div className="graph-cli">
            <div className="graph-cli__label">
              <Terminal size={16} />
              <span>
                <small>SAME GRAPH</small>
                Focus it from the CLI
              </span>
            </div>
            <code>
              <i>$</i> reposcout --graph-focus src/Http/HttpClient.php --graph-direction both
              --graph-depth 2 -f json .
            </code>
            <div className="graph-cli__formats" aria-label="Available graph output formats">
              <span>JSON</span>
              <span>DOT</span>
              <span>MERMAID</span>
            </div>
          </div>
        </div>

        <div className="graph-benefits">
          <article>
            <span>
              <Layers3 size={16} /> 01
            </span>
            <h3>Architecture before files</h3>
            <p>Drill through useful scopes without wasting clicks on empty directory layers.</p>
          </article>
          <article>
            <span>
              <Braces size={16} /> 02
            </span>
            <h3>Evidence over guesses</h3>
            <p>
              Imports and explicit extends, implements, trait, and embedding relations stay
              distinct.
            </p>
          </article>
          <article>
            <span>
              <FileCode2 size={16} /> 03
            </span>
            <h3>Built for every consumer</h3>
            <p>
              Humans explore visually; agents and automation consume bounded JSON, DOT, or
              Mermaid.
            </p>
          </article>
        </div>
      </div>
    </section>
  )
}
