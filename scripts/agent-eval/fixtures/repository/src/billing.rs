pub fn tax_for(cents: u32) -> u32 {
    cents / 5
}

pub fn invoice_total(quantity: i32, unit_price: u32) -> u32 {
    let capped = crate::inventory::cap_quantity(quantity, 100);
    let subtotal = capped as u32 * unit_price;
    subtotal + tax_for(subtotal)
}
