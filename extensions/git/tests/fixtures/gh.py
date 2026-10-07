#!/usr/bin/env python3
"""Independent GitHub CLI argument peer; no network or publishing."""
import json
import pathlib
import sys
args = sys.argv[1:]
body = None
if '--body-file' in args:
    body = pathlib.Path(args[args.index('--body-file') + 1]).read_text()
with pathlib.Path(__file__).with_suffix('.jsonl').open('a') as ledger:
    ledger.write(json.dumps({'args':args,'body':body})+'\n')
if args == ['--version']:
    print('gh version fixture')
elif args[:2] == ['auth','status']:
    print('authenticated fixture')
elif args[:2] == ['pr','create'] or args[:2] == ['pr','comment'] or args[:2] == ['repo','create']:
    print('https://github.example.test/owner/project/pull/7')
elif args[:2] == ['pr','checks']:
    if pathlib.Path(__file__).with_suffix('.legacy').exists():
        print('unknown flag: --json',file=sys.stderr)
        sys.exit(1)
    print(json.dumps([{'name':'fixture-test','state':'FAILURE','bucket':'fail'}]))
    sys.exit(1)
elif args[:2] == ['pr','view']:
    fields=args[args.index('--json')+1]
    if fields=='statusCheckRollup':print(json.dumps({'statusCheckRollup':[{'__typename':'CheckRun','name':'native-failure','status':'COMPLETED','conclusion':'FAILURE','detailsUrl':'https://checks.example.test/1'}]}))
    elif fields=='id': print(json.dumps({'id':'PR_fixture'}))
    elif fields=='comments,reviews':print(json.dumps({'comments':[{'body':'comment'}],'reviews':[{'state':'APPROVED'}]}))
    else:print(json.dumps({'number':7,'url':'https://github.example.test/owner/project/pull/7','state':'OPEN','isDraft':True,'files':[]}))
elif args[:2] == ['api','graphql']:
    query=next(v for v in args if v.startswith('query='))
    if 'viewerViewedState' in query:print(json.dumps({'data':{'node':{'files':{'nodes':[{'path':'hello.txt','viewerViewedState':'VIEWED'}],'pageInfo':{'hasNextPage':False}}}}}))
    else:print(json.dumps({'data':{'markFileAsViewed':{'clientMutationId':None}}}))
elif args[:2] == ['pr','review']:
    pass
else:
    raise SystemExit('unsupported fixture operation')
