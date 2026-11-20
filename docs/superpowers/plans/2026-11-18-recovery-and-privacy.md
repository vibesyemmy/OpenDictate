# Recovery and privacy clarity

- Test failed meeting speech clips are preserved across errors and retries, successfully recovered text is saved in timestamp order without duplicates, and files are removed only after durable recovery or meeting deletion.
- Preserve retryable clips independently of optional playback audio. Show the count and retry control on the saved meeting, run retry off the UI thread, keep failures with a clear retention policy, and mark older summaries stale after recovered text.
- Persist and display title/summary failures with manual rename and retry. Say no signal without asserting a permission denial.
- Add an honest, current privacy/data-path explanation to Settings, first meeting introduction and provider settings. Name sanitized destinations, local storage, search, retention and deletion without changing opt-in.
- Verify compatibility with prior data, test local and remote providers and failure paths, run regression tests, build, and inspect updated UI without accessing real meeting content.
