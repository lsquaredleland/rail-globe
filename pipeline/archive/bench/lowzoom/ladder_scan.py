import scan, json, sys
from multiprocessing import Pool
S=scan.load(8)
def one(name):
    R=json.load(open(f"sz_{name}.json"))
    f=lambda z,x,y: R.get(f"{z}_{x}_{y}", S.get((z,x,y),0))
    r=scan.scan(S,size_of=f)
    return name,{f"{z}|{s}":(t/1024,c) for (z,s),(t,c) in r.items()}
if __name__=="__main__":
    names=sys.argv[1:]
    with Pool(8) as p: out=dict(p.map(one,names))
    json.dump(out,open("scan_out.json","w"))
    print("worst view by zoom (KB), 1-degree grid of view centres")
    print("%-22s"%"step"+" ".join(f"z{z:<5}" for z in range(2,9))+"  max")
    for s in ("laptop","phone"):
        print(s)
        for n in names:
            v=[out[n][f"{z}|{s}"][0] for z in range(2,9)]
            print("%-22s"%n+" ".join(f"{x:6.0f}" for x in v)+f"  {max(v):6.0f}  fails:"+",".join(f"z{z}" for z,x in zip(range(2,9),v) if x>600))
