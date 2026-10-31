# Projects and activity

Projects and Activity are independent runtime capabilities. Browser renders their
State and sends commands through the existing Tembra RPC connection. Closing a panel
or a Browser client does not delete their records.

## Projects

`swarmie-projects` owns project records, conversation membership and personal folder
links. Compose its `orb(storage_factory, authorize)` with a backend-neutral `Storage`
implementation. The authorization callback selects permitted authenticated principals.
The service is `swarmie.projects`, with the `main` key.

Call `list` to start a revision-bound snapshot read. Use `page(cursor)` until the
returned cursor is absent. Join the JSON fragments before decoding the snapshot. Call `change` with a `Command` to create,
update or archive a project, assign or unassign a conversation, or replace folder links
for one project and node. Each confirmed change returns a small revision receipt. Service State contains the
committed revision, not the accumulated records. Observe it to refresh the read
projection. A newly acquired object starts at revision zero until storage is read.

Projects owns membership references. Conversations owns conversation records and
history. Folder links identify a node and retain exact paths. A link does not grant
filesystem access or add content to a model request.

Browser owns the project dialog, navigation and unadmitted draft selections. It writes
membership after conversation admission. A connection change fences pending operations.
A different node or principal clears the previous domain projection. A temporary
disconnection retains draft selections for the same node and principal.

The previous Browser project export remains in local storage. The read-only
`savedBrowserProjects(profileKey)` function can export it. When saved data exists and the native collection is empty, the Projects page shows
**Import browser projects**. This action calls `importData` to send the export to
the connected runtime. Import requires an empty native collection. It does not write
or delete the Browser export. Normal project commands do not write local storage.

## Activity

`swarmie-activity` owns the persistent inbox. Compose its
`orb(storage_factory, authorize)` independently of any UI. The service is
`swarmie.activity`, with the `main` key.

Call `list` and `page(cursor)` to read the inbox as one immutable JSON snapshot. Call `change` with `Command::Publish` to publish a
activity entry, or `Command::Read` to mark one as read. Other plugins use ordinary Tembra
References with the authenticated principal. No Browser dependency is required.

The pair of `owner` and `entity` identifies an activity entry. A publication replaces the
same pair, moves it to the front and marks it unread. Other owners and entities retain
their records. The native service supplies the update time. The optional conversation
reference supports navigation. The owner label is provenance; it does not grant
permission or establish an authenticated plugin identity.

Browser observes the inbox and renders its entries and unread indicators. Opening an
entry marks it read. Browser decides panel placement and navigation. This capability
does not deliver operating-system push notifications.

## Storage and hosting

`swarmie-storage::sqlite` supplies `SqliteProjectsStorage` and
`SqliteActivityStorage`. `SqliteOwner` supplies factories for a shared owner.
Tembra owns the storage resource at Orb scope and the per-principal service objects.
Storage remains open when clients release their service References. Host shutdown
closes the resource after dependent accesses settle.

The node composition uses `collections.sqlite` in its selected profile. Each capability
partitions records by authenticated principal. Replacements use revision checks and
commit the complete capability snapshot. A stale revision is refused. An unknown
commit requires inspection; clients do not replay the mutation. These changes do not
form a transaction with conversation history or other storage owners.

The profile is local to the runtime. Node selection determines which collection the
client reads. Hosted synchronization and shared collections across nodes are outside
this implementation. Read projection pages and compact mutation receipts use the existing Tembra
carrier. They do not change its input-frame limits.
