from tests.support import CommerceTest


class LogisticsWorkflowTests(CommerceTest):
    def ops(self, method, path, body=None):
        return self.ok(method, '/logistics' + path, body, 'a-warehouse')

    def service(self, identity='road', **changes):
        body = dict(service_id=identity, courier='Meridian Courier', name='Road parcel',
                    regions=['domestic'], base_cents=300, per_kg_cents=80,
                    fuel_basis_points=500, insurance_basis_points=100)
        body.update(changes)
        return self.ok('POST', '/logistics/services', body, 'a-admin')

    def packed(self, units=3):
        self.receive(units)
        order = self.order(units)
        order_id = order['order_id']
        self.ok('PUT', '/logistics/profiles/TEA', dict(weight_grams=250, volume_cm3=300), 'a-admin')
        wave = self.ops('POST', '/waves', dict(warehouse='north', order_ids=[order_id]))
        wave_id = wave['wave_id']
        self.ops('POST', f'/waves/{wave_id}/start')
        self.ops('POST', f'/waves/{wave_id}/pick', dict(order_id=order_id, sku='TEA', quantity=units))
        carton = self.ops('POST', f'/waves/{wave_id}/cartons',
                          dict(order_id=order_id, items=[dict(sku='TEA', quantity=units)],
                               dimensions_cm=[20, 15, 10], tare_grams=100))
        self.ops('POST', f'/cartons/{carton["carton_id"]}/seal',
                 dict(measured_grams=250 * units + 100, seal='seal-' + carton['carton_id']))
        self.ops('POST', f'/waves/{wave_id}/pack')
        return order_id, wave_id, carton['carton_id']

    def dispatched(self):
        order_id, wave_id, carton_id = self.packed()
        self.service()
        manifest = self.ops('POST', '/manifests', dict(service_id='road', warehouse='north', shipping_date='2026-10-08'))
        manifest_id = manifest['manifest_id']
        self.ops('POST', f'/manifests/{manifest_id}/cartons', dict(carton_id=carton_id))
        self.ops('POST', f'/manifests/{manifest_id}/close')
        self.ops('POST', f'/manifests/{manifest_id}/dispatch', dict(tick=10, handover_reference='handover-1'))
        return order_id, wave_id, carton_id, manifest_id

    def test_wave_claims_prevent_duplicate_work_without_moving_stock(self):
        self.receive(8)
        first = self.order(3)['order_id']
        second = self.order(5)['order_id']
        plan = self.ops('POST', '/waves/preview', dict(warehouse='north', max_units=4))
        self.assertEqual(plan['order_ids'], [first])
        self.assertEqual(plan['unit_count'], 3)
        self.assertEqual(plan['skipped'], [dict(order_id=second, reason='capacity')])
        wave = self.ops('POST', '/waves', dict(warehouse='north', order_ids=[first]))
        plan = self.ops('POST', '/waves/preview', dict(warehouse='north'))
        self.assertEqual(plan['order_ids'], [second])
        self.assertEqual(sum(row.on_hand for row in self.app.state.stock.values()), 8)
        self.ops('POST', f'/waves/{wave["wave_id"]}/cancel')
        plan = self.ops('POST', '/waves/preview', dict(warehouse='north'))
        self.assertEqual(plan['unit_count'], 8)

    def test_carton_rate_uses_dimensional_weight_rounding_and_insurance(self):
        _, _, carton_id = self.packed()
        self.service()
        self.service('too-small', max_weight_grams=500)
        self.service('slow', base_cents=100, transit_days=7)
        rates = self.ops('POST', f'/cartons/{carton_id}/rates', dict(max_transit_days=4))
        self.assertEqual(rates['recommended_service_id'], 'road')
        quote = rates['eligible'][0]
        # 20*15*10/5000 kg = 600g dimensional; actual 850g bills as one kg.
        # 300 + 80 = 380 transport; 5% = 19 fuel; ceil(3750*1%) = 38 insurance.
        self.assertEqual((quote['actual_grams'], quote['dimensional_grams'], quote['billed_kilograms']), (850, 600, 1))
        self.assertEqual(quote['total_cents'], 437)
        self.assertEqual({row['service_id']: row['reasons'] for row in rates['excluded']},
                         {'slow': ['deadline'], 'too-small': ['weight']})

    def test_dispatch_posts_existing_inventory_and_projects_customer_shipments(self):
        order_id, wave_id, carton_id, manifest_id = self.dispatched()
        document = self.ops('GET', f'/manifests/{manifest_id}')
        self.assertEqual((document['parcel_count'], document['total_grams'], document['total_cents']), (1, 850, 437))
        self.assertEqual(sum(row.on_hand for row in self.app.state.stock.values()), 0)
        self.assertEqual(sum(row.reserved for row in self.app.state.reservations.values()), 0)
        self.assertEqual(sum(row.shipped for row in self.app.state.reservations.values()), 3)
        self.assertEqual(self.ops('GET', f'/waves/{wave_id}')['status'], 'dispatched')
        result = self.ok('GET', f'/orders/{order_id}/shipments')
        self.assertEqual(result['shipped_units'], 3)
        self.assertEqual(result['outstanding_units'], 0)
        self.assertEqual(result['parcels'][0]['carton_id'], carton_id)
        self.assertNotIn('quote', result['parcels'][0])
        failure = self.raw('POST', f'/logistics/manifests/{manifest_id}/dispatch',
                           dict(tick=11, handover_reference='again'), 'a-warehouse')
        self.assertEqual(failure['status'], 409)
        self.assertEqual(len(self.app.state.shipments), 1)

    def test_delivery_exception_resolves_all_carton_shipments(self):
        order_id, _, carton_id, _ = self.dispatched()
        exception = self.ops('POST', f'/cartons/{carton_id}/exceptions',
                             dict(kind='address', description='Access code missing', tick=11))
        identity = exception['exception_id']
        self.ops('POST', f'/exceptions/{identity}/notes', dict(tick=12, text='Customer supplied code'))
        self.assertEqual(self.ok('GET', f'/orders/{order_id}/shipments')['parcels'][0]['status'], 'exception')
        self.ops('POST', f'/exceptions/{identity}/resolve', dict(tick=13, resolution='delivered', location='Reception'))
        card = self.ok('GET', f'/orders/{order_id}/shipments')['parcels'][0]
        self.assertEqual(card['status'], 'delivered')
        self.assertEqual(card['shipments'][0]['tracking'], [dict(status='delivered', location='Reception')])
        self.assertNotIn('notes', card['exceptions'][0])

    def test_courier_claim_recovery_is_not_a_customer_payment(self):
        _, _, carton_id, _ = self.dispatched()
        exception = self.ops('POST', f'/cartons/{carton_id}/exceptions',
                             dict(kind='damage', description='Water damage', tick=12))
        identity = exception['exception_id']
        claim = self.ops('POST', f'/exceptions/{identity}/claims',
                         dict(requested_cents=2500, evidence=['photo-1', 'inspection-1'], tick=13))
        claim_id = claim['claim_id']
        self.ok('POST', f'/logistics/claims/{claim_id}/decision',
                dict(approved_cents=2000, reference='decision-1', tick=14), 'a-admin')
        partial = self.ok('POST', f'/logistics/claims/{claim_id}/recover',
                          dict(amount_cents=1200, reference='bank-1', tick=15), 'a-admin')
        self.assertEqual(partial['status'], 'part_paid')
        summary = self.ok('GET', '/logistics/summary', dict(tick=16), 'a-admin')
        self.assertEqual(summary['claims_receivable_cents'], 800)
        self.assertEqual(summary['claims_recovered_cents'], 1200)
        self.assertEqual(self.app.state.payments, {})
        self.assertEqual(self.app.state.credits, {})
        paid = self.ok('POST', f'/logistics/claims/{claim_id}/recover',
                       dict(amount_cents=800, reference='bank-2', tick=16), 'a-admin')
        self.assertEqual(paid['status'], 'paid')
        self.ops('POST', f'/exceptions/{identity}/resolve', dict(resolution='claim', tick=17))

    def test_tenant_and_role_boundaries(self):
        _, wave_id, carton_id = self.packed()
        self.assertEqual(self.raw('GET', f'/logistics/waves/{wave_id}', token='b-warehouse')['status'], 404)
        self.assertEqual(self.raw('GET', f'/logistics/cartons/{carton_id}', token='a-customer')['status'], 403)
        self.assertEqual(self.raw('POST', '/logistics/services', {}, 'a-warehouse')['status'], 403)

    def test_manifest_keeps_contract_snapshot_when_service_changes(self):
        _, _, carton_id = self.packed()
        self.service()
        manifest = self.ops('POST', '/manifests', dict(service_id='road', warehouse='north', shipping_date='2026-10-08'))
        self.service(base_cents=900)
        result = self.ops('POST', f'/manifests/{manifest["manifest_id"]}/cartons', dict(carton_id=carton_id))
        self.assertEqual(result['total_cents'], 437)
        self.assertEqual(result['cartons'][0]['quote']['service_version'], 1)
        current = self.ops('POST', f'/cartons/{carton_id}/rates')
        self.assertEqual(current['eligible'][0]['total_cents'], 1067)

    def test_resealing_does_not_change_packed_carton(self):
        _, _, carton_id = self.packed()
        # A duplicate use of the seal must not alter the already packed carton.
        before = self.ops('GET', f'/cartons/{carton_id}')
        response = self.raw('POST', f'/logistics/cartons/{carton_id}/seal',
                            dict(measured_grams=2000, seal='different'), 'a-warehouse')
        self.assertEqual(response['status'], 409)
        self.assertEqual(self.ops('GET', f'/cartons/{carton_id}'), before)

    def test_failed_dispatch_rolls_back_earlier_carton_shipments(self):
        first_order, _, first_carton = self.packed(3)
        second_order, _, second_carton = self.packed(2)
        self.service()
        manifest = self.ops('POST', '/manifests', dict(service_id='road', warehouse='north', shipping_date='2026-10-08'))
        manifest_id = manifest['manifest_id']
        for carton_id in (first_carton, second_carton):
            self.ops('POST', f'/manifests/{manifest_id}/cartons', dict(carton_id=carton_id))
        self.ops('POST', f'/manifests/{manifest_id}/close')
        # An independent warehouse operator dispatches the second order first.
        self.ship(second_order, 2, warehouse='north')
        before_shipments = len(self.app.state.shipments)
        before_ledger = len(self.app.state.inventory_ledger)
        before_audit = len(self.app.state.audit)
        failure = self.raw('POST', f'/logistics/manifests/{manifest_id}/dispatch',
                           dict(tick=10, handover_reference='handover-stale'), 'a-warehouse')
        self.assertEqual(failure['status'], 409)
        self.assertEqual(len(self.app.state.shipments), before_shipments)
        self.assertEqual(len(self.app.state.inventory_ledger), before_ledger)
        self.assertEqual(len(self.app.state.audit), before_audit)
        self.assertEqual(sum(line.shipped for line in self.app.state.orders[('tenant-a', first_order)].lines.values()), 0)
        self.assertEqual(self.ops('GET', f'/cartons/{first_carton}')['status'], 'sealed')
        self.assertEqual(self.ops('GET', f'/manifests/{manifest_id}')['status'], 'closed')

    def test_completed_packing_cannot_be_voided(self):
        _, _, carton_id = self.packed()
        failure = self.raw('POST', f'/logistics/cartons/{carton_id}/void', {}, 'a-warehouse')
        self.assertEqual(failure['status'], 409)
        self.assertEqual(self.ops('GET', f'/cartons/{carton_id}')['status'], 'sealed')
