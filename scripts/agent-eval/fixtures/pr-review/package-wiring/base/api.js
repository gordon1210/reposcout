import { quote } from '#tariff';

export function dispatch(path, request) {
  if (path !== '/shipping/quote') throw new Error('unknown route');
  return { shipping_fee_cents: quote(request.region) };
}
