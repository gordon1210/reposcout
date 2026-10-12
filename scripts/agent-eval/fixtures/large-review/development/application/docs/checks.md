# Local checks

Run `python3 -B -m unittest discover -s tests` from the application root. Checks are deterministic
and use only Python's standard library. Each request-level test creates its own application;
there are no services, shared databases, network fixtures or package installs.

Checks exercise catalog visibility and search, price snapshots and arithmetic, basket versions
and checkout replay, stock conservation, partial shipment and returns, cancellations, billing,
authentication, event replay/rejection, local message retries and operational reports. Expected
amounts and quantities are literal business assertions. Tests may use focused domain functions
when the function itself is the public arithmetic policy. A review should distinguish source
inspection from commands actually executed and should report the scope of its checks.
