pub fn checkout_total(quantity: i32) -> u32 {
    crate::billing::invoice_total(quantity, 250)
}

pub fn preview_total(quantity: i32) -> u32 {
    crate::billing::invoice_total(quantity, 125)
}
