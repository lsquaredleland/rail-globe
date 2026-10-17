import math, sys, json
from pmtiles.reader import Reader, MmapSource
import os
SP=os.path.dirname(os.path.abspath(__file__))
P=os.path.join(SP,"..","..","..","app","public","data","world","world.pmtiles")
VIEWS={"Europe":(10,50),"World":(0,20),"Moscow":(37.62,55.75),"Cairo":(31.24,30.04),"Delhi":(77.2,28.6),"Mumbai":(72.87,19.07),"Seoul":(126.98,37.57),"Beijing":(116.4,39.9),"Shanghai":(121.47,31.23),"Paris":(2.35,48.86),"London":(-0.12,51.5),"Tokyo":(139.7,35.7)}
SCR={"laptop":(1440,900),"phone":(390,844)}
def wp(lon,lat,z):
    s=512*2**z; x=(lon+180)/360*s
    y=(1-math.log(math.tan(math.radians(lat))+1/math.cos(math.radians(lat)))/math.pi)/2*s
    return x,y
def tiles(lon,lat,z,w,h):
    cx,cy=wp(lon,lat,z); tz=int(math.floor(z)); sc=2**(z-tz); t=512*sc
    n=2**tz
    xs=range(int(math.floor((cx-w/2)/t)),int(math.floor((cx+w/2)/t))+1)
    ys=range(int(math.floor((cy-h/2)/t)),int(math.floor((cy+h/2)/t))+1)
    return tz,[(x%n,y) for x in xs for y in ys if 0<=y<n]
if __name__=="__main__":
    rd=Reader(MmapSource(open(P,"rb")))
    res={}
    print("view screen "+" ".join(f"z{z}" for z in range(2,9)))
    for v,(lon,lat) in VIEWS.items():
        for s,(w,h) in SCR.items():
            row=[]
            for z in range(2,9):
                tz,ts=tiles(lon,lat,z,w,h)
                b=sum(len(rd.get(tz,x,y) or b"") for x,y in set(ts))
                row.append(b/1024); res[f"{v}|{s}|{z}"]=[b/1024,[(tz,x,y,len(rd.get(tz,x,y) or b'')) for x,y in set(ts)]]
            print(f"{v:9} {s:7} "+" ".join(f"{r:6.0f}" for r in row))
    json.dump(res,open("views.json","w"))
