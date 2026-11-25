import base64,json,sqlite3,sys
from pathlib import Path
request=json.loads(base64.b64decode(sys.argv[1]).decode('utf-8'))
db=sqlite3.connect(Path(request['dbPath']).as_uri()+'?mode=ro',uri=True,timeout=5)
db.row_factory=sqlite3.Row
try:
    provider=request['provider']
    session_table='session_v2' if provider=='opencode2' else 'session'
    def columns(table):
        return {row['name'] for row in db.execute('PRAGMA table_info('+table+')')}
    session_columns=columns(session_table)
    required={'id','directory','time_created','time_updated'}
    if not required.issubset(session_columns):
        print(json.dumps({'schemaAvailable':False,'sessions':[],'messages':[]}))
        sys.exit(0)
    if provider=='opencode2':
        if not {'session_id','type','data','time_created','seq'}.issubset(columns('session_message')):
            raise ValueError('OpenCode 2 transcript schema is unreadable')
        source='session_message m'
        role='m.type'
        content='m.data'
        visible='1=1'
        order='m.time_created ASC,m.seq ASC'
    else:
        if not {'id','session_id','data','time_created'}.issubset(columns('message')) or not {'id','message_id','data','time_created'}.issubset(columns('part')):
            raise ValueError('ZCode transcript schema is unreadable')
        source='message m JOIN part p ON p.message_id=m.id'
        role="json_extract(m.data,'$.role')"
        content='p.data'
        visible="COALESCE(json_extract(m.data,'$.semantics.transcriptVisibility'),'visible')!='hidden'"
        message_sequence='m.sequence IS NULL ASC,m.sequence ASC,' if 'sequence' in columns('message') else ''
        part_sequence='p.sequence IS NULL ASC,p.sequence ASC,' if 'sequence' in columns('part') else ''
        order=message_sequence+'m.time_created ASC,m.rowid ASC,'+part_sequence+'p.time_created ASC,p.id ASC'
    title='s.title' if 'title' in session_columns else 'NULL'
    scope="(s.directory=:worktree OR substr(s.directory,1,length(:prefix))=:prefix)"
    parameters={'worktree':request['worktree'],'prefix':request['worktree'].rstrip('/\\')+request['separator'],'query':request.get('query','').lower(),'sessionId':request.get('sessionId')}
    message_search='EXISTS(SELECT 1 FROM '+source+' WHERE m.session_id=s.id AND '+visible+' AND instr(lower('+content+'),:query)>0)'
    search='(:query=\'\' OR instr(lower(COALESCE('+title+',\'\')),:query)>0 OR instr(lower(s.id),:query)>0 OR instr(lower(s.directory),:query)>0 OR '+message_search+')'
    query='SELECT s.id,s.directory,'+title+' AS title,s.time_created,s.time_updated FROM '+session_table+' s WHERE '+scope
    if request.get('sessionId') is not None:
        query+=' AND s.id=:sessionId'
    else:
        query+=' AND '+search
    query+=' ORDER BY s.time_updated DESC,s.id ASC'
    sessions=[dict(row) for row in db.execute(query,parameters)]
    messages=[]
    message_query='SELECT '+role+' AS role,'+content+' AS data,m.time_created FROM '+source+' WHERE m.session_id=:sessionId AND '+visible
    for session in sessions:
        bindings={'sessionId':session['id']}
        if provider=='opencode2':
            count_query='SELECT COUNT(*) FROM session_message m WHERE m.session_id=:sessionId'
        else:
            count_query='SELECT COUNT(*) FROM message m WHERE m.session_id=:sessionId AND '+visible
        session['message_count']=db.execute(count_query,bindings).fetchone()[0]
        first=db.execute(message_query+' AND '+role+"='user' ORDER BY "+order+' LIMIT 1',bindings).fetchone()
        session['first_user_data']=first['data'] if first else None
        if request.get('sessionId') is not None:
            messages=[dict(row) for row in db.execute(message_query+' ORDER BY '+order,bindings)]
    print(json.dumps({'schemaAvailable':True,'sessions':sessions,'messages':messages},ensure_ascii=False))
finally:
    db.close()