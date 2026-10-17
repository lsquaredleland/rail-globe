import exp, json, sys, time
LAD={
 "L0_base":{"nothing":0},
 "L1_dedupe":{"dedupe":True},
 "L2_dp4":{"dedupe":True,"dp":4},
 "L3_ln1":{"dedupe":True,"dp":4,"ln_keep":1},
 "L4_q2":{"dedupe":True,"dp":4,"ln_keep":1,"q":2},
 "L5_st":{"dedupe":True,"dp":4,"ln_keep":1,"q":2,"drop_stations_upto":5},
 "B1_dp4_q2":{"dedupe":True,"dp":4,"q":2},
 "B2_dp4_q2_st":{"dedupe":True,"dp":4,"q":2,"drop_stations_upto":5},
 "B3_dp4_regz6":{"dedupe":True,"dp":4,"thin_types":{"5":["high_speed","intercity","metro"]}},
 "B4_dp4_q2_regz6_st":{"dedupe":True,"dp":4,"q":2,"drop_stations_upto":5,"thin_types":{"5":["high_speed","intercity","metro"]}},
 "R1":{"dedupe":True,"dp":8,"ln_keep":1,"q":2,"drop_stations_upto":5},
 "R2":{"dedupe":True,"dp":12,"ln_keep":1,"q":4,"drop_stations_upto":5},
 "R3":{"dedupe":True,"dp":12,"ln_keep":1,"q":4,"drop_stations_upto":5,"thin_minln":[6,3]},
 "B5_dp8_q2_st":{"dedupe":True,"dp":8,"q":2,"drop_stations_upto":5},
 "B6_dp6_q2_st":{"dedupe":True,"dp":6,"q":2,"drop_stations_upto":5},
 "B7_dp8_q4_st":{"dedupe":True,"dp":8,"q":4,"drop_stations_upto":5},
 "B8_dp4_q2_ln3_st":{"dedupe":True,"dp":4,"q":2,"ln_keep":3,"drop_stations_upto":5},
 "B9_dp4_q2_ln5_st":{"dedupe":True,"dp":4,"q":2,"ln_keep":5,"drop_stations_upto":5},
 "B10_dp4_q2_noln_st":{"dedupe":True,"dp":4,"q":2,"dropcols":["ln"],"drop_stations_upto":5},
 "C3_corr4":{"dedupe":True,"dp":4,"dropcols":["ln","nk","oc"],"corridor":4,"q":2,"drop_stations_upto":5},
}
if __name__=="__main__":
    for n in (sys.argv[1:] or LAD):
        t=time.time(); res=exp.run(LAD[n]); json.dump({k:v["_total_gz"] for k,v in res.items()},open(f"sz_{n}.json","w"))
        print(n,len(res),"tiles",round(time.time()-t),"s")
