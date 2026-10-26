# Residential Parking Permits — high-level requirements (v0.1)

Owner: City Mobility Office. Status: draft for discussion.

## Purpose

Residents living in a controlled parking zone need a permit to park on the street in that zone.
Today applications arrive by email and are checked by hand. We want an online service where
residents apply, permit officers decide, and wardens can check whether a car has a valid permit.

## Applying

- A resident shall be able to apply for a parking permit for one vehicle.
- An application shall record the resident, the home address, the parking zone, the vehicle
  registration number and the requested start date.
- The service shall reject an application when the home address is not inside the requested
  parking zone.
- A household shall hold at most two active permits at the same time.
- A resident can save an application as a draft and submit it later.

## Deciding

- A submitted application shall be decided by a permit officer, who either approves or rejects it.
- When rejecting, the officer shall record a reason that the resident can see.
- A permit officer shall not decide an application they submitted themselves.
- Approving an application shall issue a permit that is valid for 12 months from the requested
  start date.

## Fees

- The permit fee is the zone's annual price.
- Vehicles with zero emissions get a 50% discount on the fee.
- A second permit for the same household costs 25% more than the first.
- The fee shall be paid before the permit becomes active. Unpaid approved permits expire after
  14 calendar days.

## During the permit

- A resident shall be able to change the vehicle on an active permit at most twice per year.
- A resident may cancel an active permit at any time; the unused full months are refunded.
- A permit becomes expired at the end of its validity period.
- The office shall remind the resident 30 days before the permit expires.

## Enforcement

- A traffic warden shall be able to check whether a vehicle registration has an active permit
  for a given zone.
- A warden shall see only the permit status and zone, never the resident's name or address.
- Permit officers can revoke a permit, for example when the resident moves out of the zone.
  A revoked permit is not refunded.

## Quality

- The permit check for wardens should respond quickly, also on a weak mobile connection.
- The service should be accessible to residents using screen readers.
