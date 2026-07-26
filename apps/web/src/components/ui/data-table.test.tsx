import { render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import type { ColumnDef } from "@tanstack/react-table"
import { describe, expect, it } from "vitest"

import { DataTable, DataTableColumnHeader } from "@/components/ui/data-table"

interface Entry {
  id: string
  name: string
  score: number
}

const columns: ColumnDef<Entry, unknown>[] = [
  {
    accessorKey: "name",
    header: ({ column }) => <DataTableColumnHeader column={column} title="Name" />,
    meta: { label: "Name" },
  },
  {
    accessorKey: "score",
    header: ({ column }) => <DataTableColumnHeader column={column} title="Score" align="right" />,
    meta: { label: "Score", cellClassName: "text-right" },
  },
]

function renderTable(data: Entry[], defaultPageSize = 5) {
  render(
    <DataTable
      columns={columns}
      data={data}
      label="Entries"
      searchPlaceholder="Search entries..."
      searchText={(entry) => `${entry.name} ${entry.score}`}
      defaultPageSize={defaultPageSize}
      getRowId={(entry) => entry.id}
    />,
  )
}

describe("DataTable", () => {
  it("sorts and searches rows", async () => {
    const user = userEvent.setup()
    renderTable([
      { id: "beta", name: "Beta", score: 2 },
      { id: "alpha", name: "Alpha", score: 1 },
      { id: "needle", name: "Needle", score: 3 },
    ])

    const table = screen.getByRole("table", { name: "Entries" })
    expect(within(within(table).getAllByRole("row")[1]).getByText("Beta")).toBeInTheDocument()

    await user.click(screen.getByRole("button", { name: "Sort by Name" }))
    expect(within(within(table).getAllByRole("row")[1]).getByText("Alpha")).toBeInTheDocument()

    await user.type(screen.getByRole("searchbox", { name: "Search Entries" }), "needle")
    expect(within(table).getAllByRole("row")).toHaveLength(2)
    expect(within(table).getByText("Needle")).toBeInTheDocument()
    expect(screen.getByText("1–1 of 1 rows (3 total)")).toBeInTheDocument()
  })

  it("paginates without rendering the full data set", async () => {
    const user = userEvent.setup()
    const data = Array.from({ length: 12 }, (_, index) => ({
      id: String(index + 1),
      name: `Entry ${index + 1}`,
      score: index + 1,
    }))
    renderTable(data)

    const table = screen.getByRole("table", { name: "Entries" })
    expect(within(table).getAllByRole("row")).toHaveLength(6)
    expect(screen.getByText("1–5 of 12 rows")).toBeInTheDocument()
    expect(within(table).queryByText("Entry 6")).not.toBeInTheDocument()

    await user.click(screen.getByRole("button", { name: "Go to next page" }))

    expect(screen.getByText("6–10 of 12 rows")).toBeInTheDocument()
    expect(within(table).getByText("Entry 6")).toBeInTheDocument()
    expect(within(table).queryByText("Entry 1")).not.toBeInTheDocument()
  })

  it("lets users hide and restore columns", async () => {
    const user = userEvent.setup()
    renderTable([{ id: "alpha", name: "Alpha", score: 1 }])

    await user.click(screen.getByRole("button", { name: "Columns" }))
    await user.click(screen.getByRole("menuitemcheckbox", { name: "Score" }))

    expect(screen.queryByRole("columnheader", { name: "Score" })).not.toBeInTheDocument()

    await user.click(screen.getByRole("button", { name: "Columns" }))
    await user.click(screen.getByRole("menuitemcheckbox", { name: "Score" }))

    expect(screen.getByRole("columnheader", { name: "Score" })).toBeInTheDocument()
  })
})
