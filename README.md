👋 This repository contains scripts that given a set of details about a Stellar contract to build, will attempt to build it and match it to a contract deployed on Stellar mainnet.

> [!WARNING]
> This repository is an experiment. The contents should _not_ be used at this time as an input to auditing or any financial or otherwise meaningful decisions. Use this repository only to engage in the experiment and for no other purpose.

## Verified Contracts

Any contracts that have been successfully verified are detailed in the `verifications/` repo. Each verified contract will have a JSON file named with the wasm hash.

## Verify a Contract

To have a contract run through the verification process, open an issue here:

- https://github.com/leighmcculloch/stellar-contract-verifications/issues/new?template=verification-request.yml

## Discussion and Feedback

To discuss what's happening in this repo, please go to this discussion thread:

- https://github.com/orgs/stellar/discussions/1802

## Known Limitations

- This is not audited or tested. It's an experiment, early days.
- Multiple commits in a repo may build to the same wasm hash, and this repo will only verify a contract once, the first time.
