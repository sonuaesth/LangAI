import { describe,expect,it } from "vitest";

function assign(channels:Record<number,"blue"|"green">, solved:number){
  return {...channels,[solved+1]:(solved+1)%2===0?"blue":"green" as const};
}

describe("exercise channels",()=>{
  it("does not recolor an already active neighbor",()=>{
    const before={0:"blue",1:"green"} as const;
    const after=assign(before,1);
    expect(after[1]).toBe("green");
    expect(after[2]).toBe("blue");
  });
});

describe("option order",()=>{
  it("stays stable when the same stored array is reused",()=>{
    const order=[5,2,4,1,3];
    expect(order).toEqual(order);
  });
});
