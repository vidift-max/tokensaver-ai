# First-Contact OCI VM Walkthrough

## Current action

The next physical action is performed by the administrator in the OCI Console from the Android tablet. No repository secret or credential is required for this preparation step.

## Console sequence

1. Open OCI Console.
2. Confirm the selected region is the tenancy's home region.
3. Open Compute -> Instances.
4. Select the intended compartment.
5. Choose Create instance.
6. Give the instance a recognizable name, such as phone-02-first-contact.
7. Select an ARM64 Linux image.
8. Open Change shape.
9. Select Virtual machine -> Ampere.
10. Select VM.Standard.A1.Flex.
11. Confirm the Console shows Always Free eligible.
12. Allocate no more than 2 OCPUs and 12 GB memory for the initial instance.
13. Use the smallest practical boot volume that remains Always Free eligible.
14. Configure the network according to FIRST_CONTACT_OCI_NETWORK_PLAN.md.
15. Configure SSH using an administrator-controlled public key.
16. Review the final summary and cost/Always Free indicators.
17. Do not click Create if any selected resource is not Always Free eligible or if an unexpected charge is shown.

## Capacity condition

If OCI reports that the A1 shape is out of host capacity, do not switch to a paid shape. Try another availability domain when permitted or wait for capacity to become available.

## Evidence before Create

Before final creation, capture a screenshot or record the visible configuration showing:

- region
- shape
- OCPU
- memory
- boot volume
- Always Free eligibility

Do not expose private keys or account secrets in screenshots.

## Boundary

This walkthrough does not authorize creation of the VM by an external agent. The actual Create action is performed by the account administrator.

A created VM is infrastructure evidence only; it is not evidence of Phone-01/Phone-02 communication.
