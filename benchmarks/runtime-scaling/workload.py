def source(mode, payload, socket_deadline_seconds=30):
    imports = ['bufio', 'os', 'strconv']
    if mode == 'timers':
        imports += ['context', 'time']
    if mode.startswith('socket'):
        imports += ['net', 'time']
    declaration = f'''    var payload = Array<int>{{}}
    for var i = 0; i < {payload}; i += 1 {{ payload.push(i) }}'''
    tail = f'''    if payload.len() != {payload} {{ panic("payload length") }}
    if payload.len() > 0 && payload[payload.len() - 1] != {payload - 1} {{ panic("payload value") }}
    return 1'''
    common = '''    let ready = channel<bool>()
    var tasks = Array<Task<int>>{}'''
    if mode == 'tasks':
        worker = f'''async func wait(ready channel<bool>, release channel<bool>) int {{
{declaration}
    ready.send(true)
    let _, _ = release.receive()
{tail}
}}'''
        setup = common + '''
    let release = channel<bool>()
    for var i = 0; i < count; i += 1 { tasks.push(go wait(ready, release)) }'''
        release = '''    for var i = 0; i < count; i += 1 { release.send(true) }'''
    elif mode == 'timers':
        worker = f'''async func wait(ready channel<bool>, ctx context.Context) int {{
{declaration}
    ready.send(true)
    let _, _ = ctx.Done().receive()
    if ctx.Err() == nil {{ panic("missing cancellation") }}
{tail}
}}'''
        setup = common + '''
    let parent, cancel = context.WithCancel(context.Background())
    for var i = 0; i < count; i += 1 {
        let ctx, _ = context.WithTimeout(parent, 3600 * time.Second)
        tasks.push(go wait(ready, ctx))
    }'''
        release = '    cancel()'
    else:
        worker = f'''async func wait(ready channel<bool>, conn own net.Conn) int {{
{declaration}
    ready.send(true)
    var data = Array<byte>{{0}}
    let n, err = conn.Read(data[:])
    {'if err != error("net.Read: timed out") || n != 0 { panic("missing timeout") }' if mode == 'socket-deadline' else 'if err != nil || n != 1 || data[0] != 120 { panic("read result") }'}
{tail}
}}'''
        deadline_seconds = socket_deadline_seconds if mode == 'socket-deadline' else 120
        setup = common + f'''
    let listener, listenError = net.Listen("tcp", "127.0.0.1:0")
    if listenError != nil {{ panic("listen") }}
    println("LISTEN " + listener.Addr())
    for var i = 0; i < count; i += 1 {{
        let conn, err = listener.Accept()
        if err != nil {{ panic("accept") }}
        let deadlineError = conn.SetReadDeadline(time.Now() + {deadline_seconds} * time.Second)
        if deadlineError != nil {{ panic("deadline") }}
        tasks.push(go wait(ready, conn))
    }}
    let closeError = listener.Close()
    if closeError != nil {{ panic("listener close") }}'''
        release = ''
    header = '\n'.join(f'import "std/{name}"' for name in imports)
    return 'package main\n\n' + header + f'''

{worker}

func population(count int, input mut bufio.Scanner) {{
{setup}
    for var i = 0; i < count; i += 1 {{ let _, _ = ready.receive() }}
    probe(count)
    if !input.Scan() || input.Text() != "release_" {{ panic("missing release") }}
{release}
    var completed = 0
    for tasks.len() > 0 {{
        let found, task = tasks.pop()
        if found {{ completed += task.wait() }}
    }}
    if completed != count {{ panic("task results") }}
}}

func main() {{
    var input = bufio.NewScanner(os.Stdin())
    for input.Scan() {{
        let count, err = strconv.Atoi(input.Text())
        if err != nil || count < 1 {{ panic("invalid population") }}
        probe(0)
        if !input.Scan() || input.Text() != "start___" {{ panic("missing start") }}
        population(count, input)
        quiesce(0)
        probe(0)
        for input.Scan() {{
            if input.Text() == "stop____" {{ return }}
            if input.Text() == "next____" {{ break }}
            if input.Text() != "snapshot" {{ panic("invalid control") }}
            probe(0)
        }}
    }}
}}

func probe(n int) {{}}
func quiesce(n int) {{}}
'''
