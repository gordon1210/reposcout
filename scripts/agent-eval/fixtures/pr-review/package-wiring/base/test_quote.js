import assert from 'node:assert/strict';
import { dispatch } from './api.js';

assert.equal(dispatch('/shipping/quote', { region: 'domestic' }).shipping_fee_cents, 499);
assert.equal(dispatch('/shipping/quote', { region: 'international' }).shipping_fee_cents, 999);
