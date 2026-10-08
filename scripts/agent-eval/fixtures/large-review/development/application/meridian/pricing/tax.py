from meridian.core.money import ratio_round
from meridian.core.errors import require


def tax_for(amount, taxable, region, rates):
    require(region in rates, "unsupported_region", "No tax policy for this delivery region")
    return ratio_round(amount, rates[region], 10_000) if taxable else 0


def taxed_lines(lines, region, rates):
    return [dict(line, tax_cents=tax_for(line["subtotal_cents"] - line["discount_cents"],
                                       line["taxable"], region, rates)) for line in lines]
