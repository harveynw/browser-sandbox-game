# Jobs and repairs — T16

Use the [README](../README.md) to start the server and browser. F at a cyan terminal opens available jobs; Quay offers Courier (150 credits) and Vehicle recovery (250 credits). E remains quick acceptance on foot when no car door is targeted. The north ring terminal at **[0, 0, -256]** offers the **200-credit / 120-second time trial**, requiring a car. Esc → Cancel job cancels an active objective. Menus do not stop the online world.

## Ordered time trial

The server consumes initial gate 0 at driving acceptance, then advances one ordered gate per tick using the actual controlled car's swept segment. The last gate is a distinct return-to-start index. Visiting the start or a later gate early cannot complete the trial; walking through gates cannot advance it. Wrong-way travel is permitted. The gold marker/minimap point identify only the next gate and the HUD displays its index and deadline. The courier's fixed Market route is hidden for other job types.

## Reserved recovery

Recovery is offered at both authored Quay and Civic terminals. The recovery job allocates one of eight authored secured van parking positions at East Docks, checks physical clearance, and consumes the parked pool's reserved capacity. Only its owning player may enter that mission car. MissionView privately identifies its entity and collection marker. While driving it, the target changes to the blue Quay garage at **[-220, 0, 48]**. Other vehicles do not count.

Stop the correct live van inside the five-metre garage zone below 2 m/s for 30 continuous ticks. Completion stops the van and tests actual doors against static geometry, cars and actors. The driver automatically exits through a supported clear door, receives a new control epoch, and the garage consumes/releases the mission car and reservation. No remote spawn teleport is used. If all doors are blocked the job stays active, the HUD reports a blocked exit, and reward waits until a door clears. Death/arrest/destruction/cancellation/disconnect precede success; timeout also wins over a same-tick final dwell. Reservations and cars are released on failure. A cancelled occupied car freezes while a blocked safe exit is retried, retaining its bounded slot until the driver can leave safely.

## Repair and payment

Drive into the green Ironworks repair zone at **[220, 0, -180]** with a damaged live car, at least 50 committed credits and no heat/pursuit. Stop below 0.5 m/s, press F, and remain stopped for 60 observed ticks. HUD progress is authoritative; moving, leaving the zone/car, downing, arrest or control/session replacement interrupts eligibility. The request reserves one operation per profile/car. Duplicate action IDs cannot create another debit.

After dwell, a bounded memory worker queues the uniquely keyed 50-credit debit. Only a successful commit followed by a valid original session/player/epoch/car check restores the car to its template health. If eligibility was lost while the debit was pending, a uniquely keyed 50-credit refund is queued instead. A rejoined guest's different car is never repaired. Private status distinguishes dwell, debit pending, refund pending, completion, cancellation and failure; credit changes are committed values. Compensation capacity is reserved in the bounded ledger before debit so event exhaustion cannot strand a refund. SQLite durability remains T17.

## Contracts and verification

The wire advances to **version 2** because PlayerStatus gains required nullable RepairView. Rust owns the types and generates browser declarations/schemas and cross-language fixtures. Old version-1 envelopes fail validation; both game build IDs are `t16-local`. Content and the local WASM bridge retain their separate version 1 schemas, with the city package and shared movement traces unchanged.

Eight real-codec world scenarios cover trial acceptance/order/reward/rejoin; eight-slot recovery capacity and failure cleanup; reserved-car validation and blocked garage doors; exact repair dwell/commit/idempotency; heat/funds/dwell rejection; delayed debit/disconnect/rejoin/refund without repairing either wrong car; reserved refund capacity at ledger exhaustion; and cancellation while driving with ownership/epoch release. Pure simulation-independent mission tests cover swept gate crossings/near misses, one gate per tick, distinct finish, on-foot rejection and failure/timeout precedence. Browser tests cover per-objective guidance and repair stages; generated fixtures cover each repair phase and invalid price/dwell/old protocol versions.

Final commands: `npm run check` (**209 TypeScript / 122 Rust tests, PASS**) and `npm run build` (**PASS**). Full real-RTC population performance/impairment, durable restart recovery and presentation/audio polish retain their later milestone scope. Native scenario positions and economic delay settings are test fixtures, not live gameplay bypasses.


Live production browser verification used ordinary movement and menus against the local WS/RTC server: [job picker](evidence/t16-job-picker.png), [accepted recovery](evidence/t16-recovery-active.png), and [cancelled recovery with no reward](evidence/t16-recovery-cancelled.png). Full deliveries and repair-payment edge cases use the native scenarios above; no live state injection was used.
