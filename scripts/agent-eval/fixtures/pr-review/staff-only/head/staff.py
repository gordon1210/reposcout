def discount(code, subtotal_cents):
    return 500 if code.startswith("WELCOME") else 0
