def headings(columns):
    return [
        column.replace("_", " ").title()
        for column in columns
    ]


def page(rows, number, size=20):
    start = max(number - 1, 0) * size
    end = start + size
    return rows[start:end]
