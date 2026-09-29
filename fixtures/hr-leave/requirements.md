# HR Leave Management — Pilot Requirements

## Requirements

4. **HR-001** [functional] The system shall allow an employee to create a draft leave request.
5. **HR-002** [functional] A leave request shall identify the employee, leave type, start date, and end date.
6. **HR-003** [functional] The system shall reject a leave request whose start date is after its end date.
7. **HR-004** [functional] The system shall calculate the requested leave duration in working days from the start date to the end date.
8. **HR-005** [functional] The system shall exclude Saturdays and Sundays from requested leave duration.
9. **HR-006** [functional] The system shall exclude public holidays from requested leave duration.
10. **HR-007** [functional] The system shall support a half-day leave request.
11. **HR-008** [functional] The system shall prevent submission when the employee has insufficient annual-leave balance for the requested annual leave.
12. **HR-009** [functional] Submitting a valid leave request shall change its status from Draft to Submitted.
13. **HR-010** [functional] A submitted leave request shall require a manager decision before it can become Approved or Rejected.
14. **HR-011** [functional] A manager shall be allowed to approve leave requests for employees who report directly to that manager.
15. **HR-012** [functional] A manager shall be allowed to reject leave requests for employees who report directly to that manager and shall record a rejection reason.
16. **HR-013** [security] An employee shall not approve or reject that employee's own leave request.
17. **HR-014** [functional] A contractor shall not be eligible for paid annual leave.
18. **HR-015** [functional] Approving annual leave shall deduct the approved leave duration from the employee's annual-leave balance.
19. **HR-016** [functional] Rejecting a leave request shall not change the employee's leave balance.
20. **HR-017** [functional] An employee shall be allowed to withdraw a Submitted leave request before a manager decision.
21. **HR-018** [functional] Withdrawing a leave request shall change its status to Withdrawn.
22. **HR-019** [functional] An employee shall be allowed to cancel an Approved leave request before the leave start date.
23. **HR-020** [functional] Cancelling an Approved annual-leave request shall restore the deducted annual-leave balance.
24. **HR-021** [functional] Cancelling a leave request shall change its status to Cancelled.
25. **HR-022** [functional] An approval or rejection shall record the deciding manager and decision timestamp.
26. **HR-023** [functional] The system shall notify the employee when a leave request is Approved.
27. **HR-024** [functional] The system shall notify the employee when a leave request is Rejected.
28. **HR-025** [functional] An employee shall be able to view that employee's own leave requests.
29. **HR-026** [functional] A manager shall be able to view leave requests of employees who report directly to that manager.
30. **HR-027** [functional] The employee's annual-leave balance shall never be negative.
31. **HR-028** [functional] The system shall prevent overlapping Approved leave requests for the same employee.
32. **HR-029** [constraint] Business timestamps shall use the Europe/Warsaw time zone.
33. **HR-030** [quality] The leave-request submission operation should complete within 2 seconds under normal office load.
34. **HR-031** [operational] Approval records shall be retained for five years.
35. **HR-032** [functional] The system shall create an audit record for every approval, rejection, withdrawal, and cancellation decision.

## Reference table

| Leave type | Deducts annual balance | Eligible contract |
|---|---:|---|
| Annual | yes | Employee |
| Unpaid | no | Employee, Contractor |
| Sick | no | Employee |
