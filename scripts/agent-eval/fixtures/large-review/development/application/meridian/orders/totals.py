from meridian.core.money import ratio_round


def line_value(line, units):
    gross = line.ordered * line.unit_cents - line.discount_cents + line.tax_cents
    return ratio_round(gross, units, line.ordered)


def order_totals(order):
    original_goods = sum(line_value(line, line.ordered) for line in order.lines.values())
    cancelled_goods = sum(line_value(line, line.cancelled) for line in order.lines.values())
    returned_goods = sum(line_value(line, line.returned) for line in order.lines.values())
    return {"original_cents": original_goods + order.shipping_cents,
            "cancelled_cents": cancelled_goods, "returned_cents": returned_goods,
            "net_cents": original_goods + order.shipping_cents - cancelled_goods - returned_goods}
