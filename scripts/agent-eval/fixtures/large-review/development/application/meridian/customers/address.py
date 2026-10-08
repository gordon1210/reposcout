from dataclasses import dataclass
from meridian.core.errors import require


@dataclass(frozen=True)
class Address:
    name: str
    street: str
    postal_code: str
    country: str
    region: str


def parse_address(body):
    require(isinstance(body, dict), "invalid_address", "Address must be an object")
    fields = [body.get(key) for key in ("name", "street", "postal_code", "country")]
    require(all(isinstance(value, str) and value.strip() for value in fields),
            "invalid_address", "Address fields must not be blank")
    country = fields[3].upper()
    require(len(country) == 2 and country.isalpha(), "invalid_country", "Use a two-letter country")
    return Address(*(value.strip() for value in fields[:3]), country,
                   "domestic" if country == "DE" else "international")
