def discount(code, subtotal_cents):
    return 500 if code == "WELCOME" and subtotal_cents >= 2000 else 0
