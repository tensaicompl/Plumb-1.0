# Equipment Loan Desk — requirements (draft v0.3)

Notes from the kickoff workshop. Not everything here is final; Pat still owes us the holiday list.

## Borrowing

* **CZ-01** [functional] A student shall be able to reserve an equipment item.
* CZ-02 [functional] A reservation shall identify the student, the equipment item, the pickup date, and the return date.
- **CZ-03** [functional] The desk shall reject a reservation whose pickup date is after its return date.
- **CZ-04** [functional] A visitor shall not be eligible to borrow an equipment item.

## Loan length

1) **CZ-05** [functional] The system shall calculate the loan length in working days from the pickup date to the return date.
2) **CZ-06** [functional] The system shall exclude public holidays from the loan length.
3) **CZ-07** [functional] The system shall support a half-day loan.

## Statuses

1. **CZ-08** [functional] Confirming a valid reservation shall change its status from Requested to Confirmed.
2. **CZ-09** [functional] A technician shall be allowed to hand out a Confirmed reservation.
3. **CZ-10** [functional] Handing out a reservation shall change its status to Loaned.
4. **CZ-11** [functional] A student shall be allowed to cancel a Requested reservation.
5. **CZ-12** [functional] Cancelling a reservation shall change its status to Cancelled.
6. **CZ-13** [functional] Returning an equipment item shall change the reservation status to Returned.

## Other

**CZ-14** [quality] The reservation screen should respond quickly.

- the desk might also lend phone chargers (to be discussed)

TODO: ask about late fees
