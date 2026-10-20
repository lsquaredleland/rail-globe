import json, os, sys, subprocess, glob, math, hashlib, collections
from multiprocessing import Pool
SP=os.path.dirname(os.path.abspath(__file__))
ENC=SP+"/reenc/target/release/reenc"

def dp(pts, tol):
    n=len(pts)
    if n<3 or tol<=0: return pts
    keep=[False]*n; keep[0]=keep[-1]=True
    stack=[(0,n-1)]; t2=tol*tol
    while stack:
        a,b=stack.pop()
        ax,ay=pts[a]; bx,by=pts[b]; dx=bx-ax; dy=by-ay; L2=dx*dx+dy*dy
        mx=-1; mi=-1
        for i in range(a+1,b):
            px,py=pts[i]
            if L2==0: d=(px-ax)**2+(py-ay)**2
            else:
                t=((px-ax)*dx+(py-ay)*dy)/L2
                t=0 if t<0 else (1 if t>1 else t)
                d=(px-ax-t*dx)**2+(py-ay-t*dy)**2
            if d>mx: mx=d; mi=i
        if mx>t2:
            keep[mi]=True; stack.append((a,mi)); stack.append((mi,b))
    return [p for p,k in zip(pts,keep) if k]

def dedupe(pts):
    out=[pts[0]]
    for p in pts[1:]:
        if p!=out[-1]: out.append(p)
    return out

def chain_edges(edges):
    adj=collections.defaultdict(set)
    for a,b in edges: adj[a].add(b); adj[b].add(a)
    seen=set(); lines=[]
    def ekey(a,b): return (a,b) if a<=b else (b,a)
    def walk(start,nxt):
        line=[start,nxt]; seen.add(ekey(start,nxt)); prev=start; cur=nxt
        while len(adj[cur])==2:
            n2=[x for x in adj[cur] if x!=prev]
            if not n2: break
            n=n2[0]
            if ekey(cur,n) in seen: break
            seen.add(ekey(cur,n)); line.append(n); prev,cur=cur,n
        return line
    for node in list(adj):
        if len(adj[node])!=2:
            for nb in adj[node]:
                if ekey(node,nb) not in seen: lines.append(walk(node,nb))
    for node in list(adj):
        for nb in adj[node]:
            if ekey(node,nb) not in seen: lines.append(walk(node,nb))
    return lines

def transform(d, z, P):
    g=P.get("q",1) if z<=P.get("q_upto",99) else 1
    out={"layers":[]}
    for L in d["layers"]:
        name=L["name"]; F=L["features"]; ext=L["extent"]
        if name in ("stations","complexes") and P.get("drop_stations_upto",-1)>=z:
            continue
        if name=="cities" and P.get("drop_cities"): continue
        if name in ("track","con_dash") and F:
            nf=[]
            for f in F:
                p=dict(f["p"])
                if name=="track":
                    for c in P.get("dropcols",[]): p.pop(c,None)
                    if "ln_keep" in P and "ln" in p:
                        p["ln"]=",".join(str(p["ln"]).split(",")[:P["ln_keep"]])
                    if P.get("thin_noln") and z<=P["thin_noln"] and "ln" not in f["p"] and "nk" not in f["p"]: continue
                    if "thin_types" in P and str(z) in P["thin_types"] and p.get("type") not in P["thin_types"][str(z)]: continue
                    if P.get("thin_minln") and z<=P["thin_minln"][0] and f["p"].get("type")!="high_speed" and "ln" in f["p"] and len(str(f["p"]["ln"]).split(","))<P["thin_minln"][1]: continue
                    if P.get("thin_minln") and z<=P["thin_minln"][0] and f["p"].get("type")!="high_speed" and "ln" not in f["p"]: continue
                rings=[]
                for r in f["c"]:
                    if g>1: r=[[round(x/g),round(y/g)] for x,y in r]
                    r=[tuple(p_) for p_ in r]
                    if P.get("dedupe") or g>1: r=dedupe(r)
                    if P.get("dp"): r=dp(r,P["dp"]/g)
                    if len(r)>=2: rings.append(r)
                if f["t"] in (1,4) and not rings: continue
                nf.append({"p":p,"t":(1 if len(rings)==1 else 4) if f["t"] in (1,4) else f["t"],"c":rings if f["t"] in (1,4) else f["c"]})
            F=nf
            if P.get("corridor") and name=="track":
                # union of snapped edges per attribute group; attributes must be coarse
                cg=P["corridor"]/g
                groups=collections.defaultdict(set)
                for f in F:
                    key=json.dumps(f["p"],sort_keys=True)
                    s=groups[key]
                    for r in f["c"]:
                        sp=[(round(x/cg),round(y/cg)) for x,y in r]; sp=dedupe(sp)
                        for a,b in zip(sp,sp[1:]): s.add((a,b) if a<=b else (b,a))
                F=[]
                for key,es in sorted(groups.items()):
                    lines=chain_edges(es)
                    rings=[[(int(round(x*cg)),int(round(y*cg))) for x,y in l] for l in lines]
                    F.append({"p":json.loads(key),"t":4 if len(rings)>1 else 1,"c":rings})
            elif P.get("sort") or P.get("dropcols") or "ln_keep" in P:
                F=sorted(F,key=lambda f: json.dumps(f["p"],sort_keys=True))
            if P.get("hit_split") and name=="track":
                pass
        else:
            if g>1 and name in ("stations","complexes","cities","con_dash"):
                F=[{"p":f["p"],"t":f["t"],"c":[[ [round(x/g),round(y/g)] for x,y in r] for r in f["c"]]} for f in F]
        out["layers"].append({"name":name,"extent":ext//g if g>1 else ext,"features":[{"p":f["p"],"t":f["t"],"c":[[list(p_) for p_ in r] for r in f["c"]]} for f in F]})
    return out

def work(args):
    tile,P=args
    z=int(tile.split("_")[0])
    d=json.load(open(f"{SP}/dec/{tile}.json"))
    t=transform(d,z,P)
    tmp=f"{SP}/tmp_{os.getpid()}.json"
    json.dump(t,open(tmp,"w"))
    r=json.loads(subprocess.run([ENC,tmp],capture_output=True,text=True,check=True).stdout)
    return tile,r

def run(P, tiles=None, procs=12):
    key=hashlib.md5(json.dumps(P,sort_keys=True).encode()).hexdigest()[:10]
    cache=f"{SP}/cache_{key}.json"
    if os.path.exists(cache) and tiles is None: return json.load(open(cache))
    tl=tiles or [os.path.basename(f)[:-5] for f in glob.glob(SP+"/dec/*.json")]
    with Pool(procs) as p: res=dict(p.map(work,[(t,P) for t in tl],chunksize=2))
    if tiles is None: json.dump(res,open(cache,"w"))
    return res
