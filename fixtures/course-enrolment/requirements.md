# Online Course Enrolment — product brief

Prepared by the Learning Platform team. This is a first cut; numbers are still under review.

## What we are building

Learners browse the course catalogue, enrol in courses and complete graded assignments.
Instructors publish courses and grade submissions. Administrators manage refunds.

## Enrolment

1. A learner must be able to enrol in a published course.
2. A course has a capacity; enrolment must be refused when the course is full.
3. Learners who are not logged in can browse the catalogue but cannot enrol.
4. A learner can be enrolled in at most five active courses at a time.
5. The enrolment price is the course list price; members of the premium plan pay nothing.

## Assignments and grading

Each course has one or more assignments, each with a weight in percent. The weights of a
course always add up to 100. A learner submits an assignment before its due date; a late
submission is accepted for up to three days and loses 10% of its score per late day.

- The instructor of the course grades each submission with a score from 0 to 100.
- The final grade is the weighted average of the assignment scores, rounded to one decimal.
- A learner passes the course with a final grade of 60 or more.
- An instructor must not grade their own submission.

## Course lifecycle

A course starts as a draft, is published by its instructor, and is archived by an
administrator. Archived courses accept no new enrolments. Learners keep access to archived
courses they completed.

## Refunds

- A learner may withdraw within 14 days of enrolling and receives a full refund.
- After 14 days no refund is given.
- Only an administrator can approve a refund outside these rules.

## Notifications

The platform sends a reminder two days before each due date and a message when a grade is
published.
