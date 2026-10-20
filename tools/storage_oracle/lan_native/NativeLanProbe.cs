using System;
using System.Runtime.InteropServices;
using System.Text;
using System.IO;

// Original LAN callbacks remain unmodified except the six declared transport
// entries. This fixture requires an explicitly marked isolated game copy.
class NativeLanProbe {
 static string Root, ImagePath;
 delegate bool E(IntPtr h,IntPtr p);
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
 [DllImport("user32.dll")] static extern int GetDlgCtrlID(IntPtr h);
 [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr h,E e,IntPtr p);
 [DllImport("user32.dll",EntryPoint="SendMessageW")] static extern IntPtr SendRaw(IntPtr h,uint m,IntPtr w,IntPtr l);
 [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint rights,bool inherit,uint pid);
 [DllImport("kernel32.dll")] static extern IntPtr VirtualAllocEx(IntPtr process,IntPtr address,UIntPtr size,uint type,uint protect);
 [DllImport("kernel32.dll")] static extern bool WriteProcessMemory(IntPtr process,IntPtr address,byte[] bytes,UIntPtr size,out UIntPtr written);
 [DllImport("kernel32.dll")] static extern bool VirtualFreeEx(IntPtr process,IntPtr address,UIntPtr size,uint type);
 [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool CreateProcessW(string application,StringBuilder command,IntPtr processSecurity,IntPtr threadSecurity,bool inherit,uint flags,IntPtr env,string directory,ref SI startup,out PI process);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool ReadProcessMemory(IntPtr process,IntPtr address,byte[] bytes,UIntPtr size,out UIntPtr read);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool VirtualProtectEx(IntPtr process,IntPtr address,UIntPtr size,uint protection,out uint old);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool FlushInstructionCache(IntPtr process,IntPtr address,UIntPtr size);
 [DllImport("kernel32.dll",SetLastError=true)] static extern uint ResumeThread(IntPtr thread);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool TerminateProcess(IntPtr process,uint exit);
 [DllImport("kernel32.dll")] static extern uint WaitForSingleObject(IntPtr process,uint timeout);
 [DllImport("kernel32.dll")] static extern bool GetExitCodeProcess(IntPtr process,out uint exit);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool QueryFullProcessImageNameW(IntPtr process,uint flags,StringBuilder name,ref uint size);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetProcessTimes(IntPtr process,out FT created,out FT exit,out FT kernel,out FT user);
 [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)] struct SI { public int cb; public string reserved,desktop,title; public uint x,y,xsize,ysize,xchars,ychars,fill,flags; public ushort show,reserved2; public IntPtr extra,input,output,error; }
 [StructLayout(LayoutKind.Sequential)] struct PI { public IntPtr process,thread; public uint pid,tid; }
 [StructLayout(LayoutKind.Sequential)] struct FT { public uint lo,hi; public long Value(){return ((long)hi<<32)|lo;} }
 static Exception Error(string message){return new Exception(message+" win32="+Marshal.GetLastWin32Error());}
 static string Hex(byte[] b){return BitConverter.ToString(b).Replace("-","").ToLowerInvariant();}
 static void Identity(IntPtr p){var name=new StringBuilder(1024);uint n=(uint)name.Capacity;if(!QueryFullProcessImageNameW(p,0,name,ref n))throw Error("Image query");if(!String.Equals(name.ToString(),ImagePath,StringComparison.OrdinalIgnoreCase))throw new Exception("Wrong target "+name);}
 static byte[] Read(IntPtr p,uint a,int count){byte[] b=new byte[count];UIntPtr n;if(!ReadProcessMemory(p,new IntPtr(a),b,new UIntPtr((uint)count),out n)||n.ToUInt64()!=(ulong)count)throw Error("Read "+a.ToString("X"));return b;}
 static void Write(IntPtr p,uint a,byte[] b){UIntPtr n;if(!WriteProcessMemory(p,new IntPtr(a),b,new UIntPtr((uint)b.Length),out n)||n.ToUInt64()!=(ulong)b.Length)throw Error("Write "+a.ToString("X"));if(Hex(Read(p,a,b.Length))!=Hex(b))throw new Exception("Write readback mismatch");}
 static uint Allocate(IntPtr p,byte[] code){IntPtr a=VirtualAllocEx(p,IntPtr.Zero,new UIntPtr((uint)code.Length),0x3000,4);if(a==IntPtr.Zero||a.ToInt64()>UInt32.MaxValue)throw Error("Allocate 32bit stub");uint at=(uint)a.ToInt64();Write(p,at,code);uint old;if(!VirtualProtectEx(p,a,new UIntPtr((uint)code.Length),0x20,out old)||!FlushInstructionCache(p,a,new UIntPtr((uint)code.Length)))throw Error("Protect stub");return at;}
 static void Patch(IntPtr p,uint at,string expected,uint target){byte[] original=Read(p,at,expected.Length/2);if(Hex(original)!=expected)throw new Exception("Unexpected original bytes "+at.ToString("X")+" "+Hex(original));byte[] jump=new byte[original.Length];jump[0]=0xe9;Array.Copy(BitConverter.GetBytes(unchecked(target-at-5)),0,jump,1,4);for(int i=5;i<jump.Length;i++)jump[i]=0x90;uint old,unused;if(!VirtualProtectEx(p,new IntPtr(at),new UIntPtr((uint)jump.Length),0x40,out old))throw Error("Protect original");Write(p,at,jump);if(!VirtualProtectEx(p,new IntPtr(at),new UIntPtr((uint)jump.Length),old,out unused)||!FlushInstructionCache(p,new IntPtr(at),new UIntPtr((uint)jump.Length)))throw Error("Flush patch");Console.WriteLine("patch at="+at.ToString("X8")+" original="+expected+" installed="+Hex(jump)+" stub="+target.ToString("X8"));}
 static void LaunchLan(){
  var si=new SI();si.cb=Marshal.SizeOf(si);PI pi;if(!CreateProcessW(ImagePath,new StringBuilder("\""+ImagePath+"\""),IntPtr.Zero,IntPtr.Zero,false,4,IntPtr.Zero,Path.GetDirectoryName(ImagePath),ref si,out pi))throw Error("Create suspended");bool resumed=false;
  try{Identity(pi.process);if(Hex(Read(pi.process,0x400000,2))!="4d5a")throw new Exception("Wrong mapped base");
   IntPtr data=VirtualAllocEx(pi.process,new IntPtr(0x10000000),new UIntPtr(0x201000),0x3000,4);if(data.ToInt64()!=0x10000000)throw Error("Fixed mailbox allocation");
   Write(pi.process,0x10000000,BitConverter.GetBytes(0x314e414c));Write(pi.process,0x10000004,BitConverter.GetBytes(1));Write(pi.process,0x10000030,BitConverter.GetBytes(0x5e00f9));
   IntPtr privateData=VirtualAllocEx(pi.process,new IntPtr(0x11000000),new UIntPtr(0x1001000),0x3000,4);if(privateData.ToInt64()!=0x11000000)throw Error("Fixed private mailbox allocation");
   Write(pi.process,0x11000000,BitConverter.GetBytes(0x3250524c));Write(pi.process,0x11000004,BitConverter.GetBytes(1));
   string hooks=Path.Combine(Root,"hooks")+Path.DirectorySeparatorChar;
   uint open=Allocate(pi.process,File.ReadAllBytes(hooks+"open.bin"));
   uint service=Allocate(pi.process,File.ReadAllBytes(hooks+"service.bin"));
   uint send=Allocate(pi.process,File.ReadAllBytes(hooks+"send.bin"));
   uint receive=Allocate(pi.process,File.ReadAllBytes(hooks+"receive.bin"));
   uint privateSend=Allocate(pi.process,File.ReadAllBytes(hooks+"private_send.bin"));
   uint privateReceive=Allocate(pi.process,File.ReadAllBytes(hooks+"private_receive.bin"));
   long created=Creation(pi.process);File.WriteAllText(Manifest(pi.pid),created.ToString());Console.WriteLine("owned creation="+created+" mailbox=10000000 outgoing-limit=4096");
   Patch(pi.process,0x53f540,"8b0d28768800",open);Patch(pi.process,0x541820,"81ecb8040000",service);Patch(pi.process,0x5410f0,"568bf18a4604",send);Patch(pi.process,0x541370,"8a410484c0",receive);
   Patch(pi.process,0x5414c0,"5355568bf1",privateSend);Patch(pi.process,0x5415f0,"515355568bf1",privateReceive);
   Console.WriteLine("native-lan-smoke pid="+pi.pid+" tid="+pi.tid+" image="+ImagePath+" base=00400000 suspended=true excluded=open/service/global-send/global-receive/private-send/private-receive inbox=scripted-bounded outgoing=captured");
   uint previous=ResumeThread(pi.thread);if(previous!=1)throw new Exception("Unexpected suspend count "+previous);resumed=true;
   uint waited=WaitForSingleObject(pi.process,0xffffffff);uint exit;GetExitCodeProcess(pi.process,out exit);Console.WriteLine("owned child exited wait="+waited+" exit="+exit);
  }finally{if(!resumed)TerminateProcess(pi.process,1);CloseHandle(pi.thread);CloseHandle(pi.process);}
 }
 static long Creation(IntPtr process){FT created,exit,kernel,user;if(!GetProcessTimes(process,out created,out exit,out kernel,out user))throw Error("Creation time");return created.Value();}
 static string Manifest(uint pid){return Path.Combine(Root,"lan-owned-"+pid+".txt");}
 static void Owned(IntPtr process,uint pid){Identity(process);if(Creation(process)!=Int64.Parse(File.ReadAllText(Manifest(pid))))throw new Exception("Owned process creation changed");if(Hex(Read(process,0x10000000,4))!="4c414e31")throw new Exception("Wrong mailbox identity");}
 static uint R32(IntPtr process,uint address){return BitConverter.ToUInt32(Read(process,address,4),0);}
 static void Publish(IntPtr p,byte[] packet,byte[] sender,uint caller){
  if(R32(p,0x10000024)!=0)throw new Exception("Mailbox already occupied");
  bool progress=packet.Length==51&&BitConverter.ToUInt32(packet,0)==0x0f&&(caller==0x48d552||caller==0x48db00);
  bool shell=packet.Length==455&&(caller==0x5e00f9||caller==0x5dd6b3||caller==0x693de1||caller==0x6943cf);
  if((!progress&&!shell)||sender.Length!=12)throw new Exception("Invalid fixture size/type/caller");uint seq=R32(p,0x10000028)+1;
  Write(p,0x10000030,BitConverter.GetBytes(caller));Write(p,0x10000040,BitConverter.GetBytes(packet.Length));Write(p,0x10000044,sender);Write(p,0x10000050,new byte[]{4,0xaa});Write(p,0x10000054,packet);Write(p,0x10000028,BitConverter.GetBytes(seq));UIntPtr n;if(!WriteProcessMemory(p,new IntPtr(0x10000024),BitConverter.GetBytes(1),new UIntPtr(4),out n)||n.ToUInt64()!=4)throw Error("Publish ready");if(R32(p,0x10000024)!=1&&R32(p,0x1000002c)!=seq)throw new Exception("Ready/consumed mismatch");
 }
 // Controlled input through the existing receive stub, not a file-I/O or completion patch.
 // Original693E72 reads FILE_INFO+AF size;693E60 copies+B3 filename.
 // Original6943CA receives at return6943CF;694480... copies chunk index+18,size+1A,data+1C.
 static void AwaitConsumed(IntPtr p,uint pid,System.Diagnostics.Stopwatch watch) {
  while(R32(p,0x10000024)!=0) {if(watch.ElapsedMilliseconds>120000||WaitForSingleObject(p,0)!=0x102)throw new Exception("File fixture receive timeout");System.Threading.Thread.Sleep(1);}
  Owned(p,pid);
 }
 static void ServeFile(string[] args) {
  uint pid=UInt32.Parse(args[1]);byte[] data=File.ReadAllBytes(args[2]),sender=File.ReadAllBytes(args[3]);string name=args[4];
  if(data.Length<1||data.Length>4000000||sender.Length!=12||name.Length>128||name.IndexOfAny(new char[]{'/', '\\', ':'})>=0||name.Contains(".."))throw new Exception("Invalid bounded file fixture");
  foreach(char c in name)if(c<32||c>126)throw new Exception("ASCII fixture name required");
  IntPtr p=OpenProcess(0x101038,false,pid);if(p==IntPtr.Zero)throw Error("Open file target");
  try {
   Owned(p,pid);var watch=System.Diagnostics.Stopwatch.StartNew();uint cursor=R32(p,0x10000008);bool info=false,ack=false;
   while(!ack) {
    if(watch.ElapsedMilliseconds>120000||WaitForSingleObject(p,0)!=0x102)throw new Exception("File request/ack timeout");
    uint count=R32(p,0x10000008);if(count>4096||R32(p,0x10000010)!=0)throw new Exception("File transcript bound");
    while(cursor<count) {uint row=0x10001000+cursor*512;if(R32(p,row+44)!=cursor+1)break;uint type=R32(p,row+48);cursor++;
     if(type==0x10&&!info) {byte[] packet=new byte[455];Array.Copy(BitConverter.GetBytes(0x11),packet,4);Array.Copy(BitConverter.GetBytes(data.Length),0,packet,0xaf,4);Array.Copy(Encoding.ASCII.GetBytes(name),0,packet,0xb3,name.Length);AwaitConsumed(p,pid,watch);Publish(p,packet,sender,0x693de1);info=true;Console.WriteLine("native request="+(cursor-1)+" declared info size="+data.Length+" name="+name);}
     if(type==0x15&&info){ack=true;Console.WriteLine("native FILE_INFO_ACK="+(cursor-1));}
    }
    System.Threading.Thread.Sleep(1);
   }
   int total=(data.Length+191)/192;
   for(int i=0;i<total;i++) {AwaitConsumed(p,pid,watch);byte[] packet=new byte[455];Array.Copy(BitConverter.GetBytes(0x12),packet,4);int length=Math.Min(192,data.Length-i*192);Array.Copy(BitConverter.GetBytes((ushort)i),0,packet,0x18,2);Array.Copy(BitConverter.GetBytes((ushort)length),0,packet,0x1a,2);Array.Copy(data,i*192,packet,0x1c,length);Publish(p,packet,sender,0x6943cf);System.Threading.Thread.Sleep(50);if(i==total/2){Console.WriteLine("native transfer halfway; holding20s for GUI capture");System.Threading.Thread.Sleep(20000);}}
   AwaitConsumed(p,pid,watch);Console.WriteLine("declared transfer consumed chunks="+total+" bytes="+data.Length);
  }finally{CloseHandle(p);}
 }
 static void ServeAdvertisement(string[] args){uint pid=UInt32.Parse(args[1]);IntPtr p=OpenProcess(0x101038,false,pid);if(p==IntPtr.Zero)throw Error("Open ad target");try{Owned(p,pid);byte[] packet=File.ReadAllBytes(args[2]),sender=File.ReadAllBytes(args[3]);uint timeout=UInt32.Parse(args[4]);if(timeout>600000)throw new Exception("Fixture duration limit");byte[] response=args.Length>5?File.ReadAllBytes(args[5]):null;uint cursor=R32(p,0x10000008),emitted=0;var watch=System.Diagnostics.Stopwatch.StartNew();Publish(p,packet,sender,0x5e00f9);while(watch.ElapsedMilliseconds<timeout&&WaitForSingleObject(p,0)==0x102){uint count=R32(p,0x10000008);if(count>4096||R32(p,0x10000010)!=0)throw new Exception("Fixture transcript failure");while(cursor<count){uint row=0x10001000+cursor*512;if(R32(p,row+44)!=cursor+1)break;uint type=R32(p,row+48);cursor++;if(type==6&&response!=null&&R32(p,0x10000024)==0){Publish(p,response,sender,0x5e00f9);Console.WriteLine("response to native join="+(cursor-1)+" type="+BitConverter.ToUInt32(response,0));if(BitConverter.ToUInt32(response,0)==7)return;}if((type==0||type==2)&&R32(p,0x10000024)==0){Publish(p,packet,sender,0x5e00f9);emitted++;Console.WriteLine("ad reply to native outgoing="+(cursor-1)+" type="+type+" emitted="+emitted);}}System.Threading.Thread.Sleep(20);}Console.WriteLine("fixture ad-server exit replies="+emitted+" cursor="+cursor);}finally{CloseHandle(p);}}
 class Peer {public string Name;public byte[] Endpoint;public uint Connection;}
 static Peer[] ReadPeers(string path) {
  var peers=new System.Collections.Generic.List<Peer>();
  foreach(string line in File.ReadAllLines(path)) {
   if(line.Length==0)continue;
   string[] fields=line.Split('\t');if(fields.Length!=2||fields[0].Length<1||fields[0].Length>19)throw new Exception("Peer TSV requires NAME<TAB>ENDPOINT_FILE");
   foreach(char c in fields[0])if(c<32||c>126)throw new Exception("Peer name must be ASCII");
   byte[] endpoint=File.ReadAllBytes(fields[1]);if(endpoint.Length!=12)throw new Exception("Peer endpoint must be12 bytes");
   foreach(Peer old in peers)if(String.Equals(old.Name,fields[0],StringComparison.OrdinalIgnoreCase)||Hex(old.Endpoint)==Hex(endpoint))throw new Exception("Duplicate declared peer");
   peers.Add(new Peer{Name=fields[0],Endpoint=endpoint});
  }
  if(peers.Count<1||peers.Count>7)throw new Exception("Declare1..7 remote peers");return peers.ToArray();
 }
 // 697B70 creates real connections from source nodes1..N, using node+6F.
 // These reads validate identity; the fixture never changes roster/House/connection state.
 static void BindPeers(IntPtr p,Peer[] peers,bool connections) {
  uint count=R32(p,0xa8da84),nodes=R32(p,0xa8da78);
  if(count!=(uint)peers.Length+1||nodes==0)throw new Exception("Native source roster does not match declared peers");
  var seen=new System.Collections.Generic.HashSet<string>(StringComparer.OrdinalIgnoreCase);
  for(uint i=1;i<count;i++) {
   uint node=R32(p,nodes+i*4);string name=Encoding.Unicode.GetString(Read(p,node,40)).Split('\0')[0];Peer peer=null;
   foreach(Peer candidate in peers)if(candidate.Name==name)peer=candidate;
   if(peer==null||!seen.Add(name)||Hex(Read(p,node+0x28,12))!=Hex(peer.Endpoint))throw new Exception("Native peer name/endpoint mismatch at source"+i);
   if(connections) {
    uint id=R32(p,node+0x6f);if(id>7)throw new Exception("Invalid native House/connection ID");
    uint connectionCount=R32(p,0xa8ea04);bool found=false;
    if(connectionCount!=(uint)peers.Length)throw new Exception("Original CreateConnections has not established all peers");
    for(uint j=0;j<connectionCount;j++){uint connection=R32(p,0xa8e9e8+j*4);if(connection!=0&&R32(p,connection+0x64)==id)found=true;}
    if(!found)throw new Exception("Source peer has no native connection");peer.Connection=id;
    Console.WriteLine("native binding source="+i+" name="+name+" connection="+id+" endpoint="+Hex(peer.Endpoint));
   }
  }
 }
 static void FixtureAlive(IntPtr p,uint pid,System.Diagnostics.Stopwatch watch,uint timeout) {
  if(watch.ElapsedMilliseconds>=timeout||WaitForSingleObject(p,0)!=0x102)throw new Exception("Bounded game fixture expired or target exited");
  Owned(p,pid);
  foreach(uint header in new uint[]{0x10000000,0x11000000})if(R32(p,header+8)>4096||R32(p,header+12)!=0||R32(p,header+16)!=0)throw new Exception("Native transcript capacity/length failure");
 }
 static void PublishPrivate(IntPtr p,byte[] packet,uint sender) {
  if(Hex(Read(p,0x11000000,4))!="4c525032"||R32(p,0x11000024)!=0)throw new Exception("Private mailbox identity/busy");
  if(packet.Length!=14||packet[0]!=0x19||packet[11]!=0||packet[12]!=0||sender>7||packet[2]!=sender)throw new Exception("Only declared empty peer FrameSync is permitted");
  uint seq=R32(p,0x11000028)+1;
  Write(p,0x11000040,BitConverter.GetBytes(14));Write(p,0x11000044,BitConverter.GetBytes(sender));Write(p,0x11000048,packet);Write(p,0x11000028,BitConverter.GetBytes(seq));
  UIntPtr n;if(!WriteProcessMemory(p,new IntPtr(0x11000024),BitConverter.GetBytes(1),new UIntPtr(4),out n)||n.ToUInt64()!=4)throw Error("Private ready publication");
  if(R32(p,0x11000024)!=1&&R32(p,0x1100002c)!=seq)throw new Exception("Private ready/consumed readback");
 }
 // Loading: original69AE90 emits NET_PROGRESS0F, consumed48D552/48DB00.
 // First/periodic wait:648929->64A000 emits14-byte FrameSync19 at64A093.
 // Replies retain the original frame/CRC/MaxAhead but declare no remote commands.
 // Original648710->64A0C0 performs all readiness/CRC/credit checks and EXIT stays local.
 static void ServeGame(string[] args) {
  uint pid=UInt32.Parse(args[1]),timeout=UInt32.Parse(args[3]),maxFrame=UInt32.Parse(args[4]);Peer[] peers=ReadPeers(args[2]);
  if(timeout<1||timeout>600000||maxFrame<1||maxFrame>3000)throw new Exception("Game fixture bounds:1..600000ms,1..3000 frames");
  IntPtr p=OpenProcess(0x101038,false,pid);if(p==IntPtr.Zero)throw Error("Open game target");
  try {
   Owned(p,pid);if(Hex(Read(p,0x11000000,4))!="4c525032")throw new Exception("Launch with current private fixture hooks");
   var watch=System.Diagnostics.Stopwatch.StartNew();uint globalCursor=0,privateCursor=0;bool progressed=false,bound=false;
   while(true) {
    FixtureAlive(p,pid,watch,timeout);
    if(bound&&R32(p,0xa8ea04)==0){Write(p,0x11000024,BitConverter.GetBytes(0));Console.WriteLine("Original connection teardown observed; physical return still requires GUI capture");return;}
    uint globalCount=R32(p,0x10000008);
    while(globalCursor<globalCount) {
     uint at=0x10001000+globalCursor*512;if(R32(p,at+44)!=globalCursor+1)break;globalCursor++;
     if(!progressed&&R32(p,at+4)==51&&R32(p,at+48)==0x0f) {
      if(R32(p,0xa8b238)!=3||Read(p,0xa8d60e,1)[0]!=0)throw new Exception("Progress producer outside original LAN loading");
      BindPeers(p,peers,false);
      foreach(Peer peer in peers) {
       while(R32(p,0x10000024)!=0){FixtureAlive(p,pid,watch,timeout);System.Threading.Thread.Sleep(1);}
       byte[] packet=new byte[51];Array.Copy(BitConverter.GetBytes(0x0f),packet,4);byte[] name=Encoding.ASCII.GetBytes(peer.Name);Array.Copy(name,0,packet,4,name.Length);Array.Copy(BitConverter.GetBytes(100),0,packet,0x2f,4);
       Publish(p,packet,peer.Endpoint,0x48d552);Console.WriteLine("declared progress100 name="+peer.Name+" native-trigger="+(globalCursor-1));
      }
      while(R32(p,0x10000024)!=0){FixtureAlive(p,pid,watch,timeout);System.Threading.Thread.Sleep(1);}progressed=true;
     }
    }
    uint privateCount=R32(p,0x11000008);
    while(privateCursor<privateCount) {
     uint at=0x11001000+privateCursor*4096;if(R32(p,at+16)!=privateCursor+1)break;privateCursor++;
     if(R32(p,at)!=0x64a093||R32(p,at+4)!=14)continue;
     byte[] original=Read(p,at+32,14);if(original[0]!=0x19)throw new Exception("Unexpected native64A000 packet");
     if(!progressed)throw new Exception("Private synchronization preceded loading progress fixture");
     if(!bound){BindPeers(p,peers,true);bound=true;}
     uint frame=BitConverter.ToUInt32(original,3);if(frame>maxFrame)throw new Exception("Declared frame bound reached; stop or physically abort owned game");
     foreach(Peer peer in peers) {
      while(R32(p,0x11000024)!=0){FixtureAlive(p,pid,watch,timeout);if(R32(p,0xa8ea04)==0){Write(p,0x11000024,BitConverter.GetBytes(0));Console.WriteLine("Original teardown during response");return;}System.Threading.Thread.Sleep(1);}
      byte[] reply=(byte[])original.Clone();reply[2]=(byte)peer.Connection;reply[11]=0;reply[12]=0;PublishPrivate(p,reply,peer.Connection);
      Console.WriteLine("declared empty FrameSync native-record="+(privateCursor-1)+" peer="+peer.Name+" frame="+frame+" crc="+BitConverter.ToUInt32(reply,7).ToString("X8")+" max-ahead="+reply[13]);
     }
    }
    System.Threading.Thread.Sleep(1);
   }
  }finally{CloseHandle(p);}
 }
 static void Mailbox(string[] args){uint pid=UInt32.Parse(args[1]);IntPtr p=OpenProcess(0x1038,false,pid);if(p==IntPtr.Zero)throw Error("Open mailbox target");try{Owned(p,pid);string action=args[2];if(action=="dump"){
  string root=args[3];Directory.CreateDirectory(root);uint count=R32(p,0x10000008);File.WriteAllBytes(Path.Combine(root,"mailbox-state.bin"),Read(p,0x10000000,0x220));
  Console.WriteLine("send-count="+count+" overflow="+R32(p,0x1000000c)+" bad-length="+R32(p,0x10000010)+" receive-calls="+R32(p,0x10000014)+" wrong-caller="+R32(p,0x10000018)+" service-calls="+R32(p,0x1000001c)+" inbox-ready="+R32(p,0x10000024)+" published="+R32(p,0x10000028)+" consumed="+R32(p,0x1000002c));
  if(count>4096||R32(p,0x1000000c)!=0||R32(p,0x10000010)!=0)throw new Exception("Fixture capacity/length failure");for(uint i=0;i<count;i++){uint at=0x10001000+i*512;byte[] row=Read(p,at,512);if(BitConverter.ToUInt32(row,44)!=i+1)throw new Exception("Unpublished outgoing record "+i);File.WriteAllBytes(Path.Combine(root,"outgoing-"+i.ToString("D4")+".bin"),row);}Console.WriteLine("dump="+root);
  if(Hex(Read(p,0x11000000,4))=="4c525032") {
   uint privateCount=R32(p,0x11000008);File.WriteAllBytes(Path.Combine(root,"private-mailbox-state.bin"),Read(p,0x11000000,0x60));
   if(privateCount>4096||R32(p,0x1100000c)!=0||R32(p,0x11000010)!=0)throw new Exception("Private transcript capacity/length failure");
   for(uint i=0;i<privateCount;i++){byte[] row=Read(p,0x11001000+i*4096,4096);if(BitConverter.ToUInt32(row,16)!=i+1)throw new Exception("Private record unpublished");File.WriteAllBytes(Path.Combine(root,"private-outgoing-"+i.ToString("D4")+".bin"),row);}
   Console.WriteLine("private-send="+privateCount+" receive="+R32(p,0x11000014)+" wrong-caller="+R32(p,0x11000018)+" published="+R32(p,0x11000028)+" consumed="+R32(p,0x1100002c));
  }
 }else if(action=="publish"){
  byte[] packet=File.ReadAllBytes(args[3]),sender=File.ReadAllBytes(args[4]);uint caller=Convert.ToUInt32(args[5],16);
  Publish(p,packet,sender,caller);Console.WriteLine("published="+R32(p,0x10000028)+" type="+BitConverter.ToUInt32(packet,0)+" caller="+caller.ToString("X"));
 }else throw new Exception("Unknown mailbox action");}finally{CloseHandle(p);}}
 static void StopOwned(uint pid){IntPtr p=OpenProcess(0x101039,false,pid);if(p==IntPtr.Zero)throw Error("Open owned stop");try{Owned(p,pid);if(!TerminateProcess(p,0))throw Error("Terminate owned fixture");if(WaitForSingleObject(p,5000)!=0)throw new Exception("Owned fixture did not exit");Console.WriteLine("owned fixture stopped pid="+pid);}finally{CloseHandle(p);}}
 static void LanName(uint pid,IntPtr root,string text){uint owner;GetWindowThreadProcessId(root,out owner);if(owner!=pid||text.Length>16)throw new Exception("Wrong target or oversized name");IntPtr p=OpenProcess(0x101038,false,pid);if(p==IntPtr.Zero)throw Error("Open owned text target");try{Owned(p,pid);IntPtr edit=IntPtr.Zero;EnumChildWindows(root,delegate(IntPtr h,IntPtr unused){if(GetDlgCtrlID(h)==0x541){if(edit!=IntPtr.Zero)throw new Exception("Multiple name controls");edit=h;}return true;},IntPtr.Zero);if(edit==IntPtr.Zero)throw new Exception("No BB name edit");SendRaw(edit,0x100,new IntPtr(0x24),IntPtr.Zero);for(int i=0;i<32;i++)SendRaw(edit,0x100,new IntPtr(0x2e),IntPtr.Zero);foreach(char c in text)SendRaw(edit,0x102,new IntPtr(c),IntPtr.Zero);IntPtr buffer=VirtualAllocEx(p,IntPtr.Zero,new UIntPtr(40),0x3000,4);if(buffer==IntPtr.Zero)throw Error("Text read buffer");try{SendRaw(edit,0x4b3,new IntPtr(20),buffer);string actual=Encoding.Unicode.GetString(Read(p,(uint)buffer.ToInt64(),40)).Split('\0')[0];Console.WriteLine("native NewEdit name="+actual+" supplied="+text);if(actual!=text)throw new Exception("Native text readback mismatch");}finally{VirtualFreeEx(p,buffer,UIntPtr.Zero,0x8000);}}finally{CloseHandle(p);}}
 static void Main(string[] args) {
  Root=Environment.GetEnvironmentVariable("VERA20K_NATIVE_FIXTURE_ROOT");
  ImagePath=Environment.GetEnvironmentVariable("VERA20K_NATIVE_IMAGE");
  if(String.IsNullOrEmpty(Root)||String.IsNullOrEmpty(ImagePath))throw new Exception("Set explicit isolated VERA20K_NATIVE_FIXTURE_ROOT and VERA20K_NATIVE_IMAGE");
  Root=Path.GetFullPath(Root);ImagePath=Path.GetFullPath(ImagePath);
  if(!File.Exists(Path.Combine(Path.GetDirectoryName(ImagePath),".vera20k-native-fixture")))throw new Exception("Unmarked isolated game copy");
  if(args.Length==1&&args[0]=="launch-lan"){LaunchLan();return;}
  if(args.Length==2&&args[0]=="stop-owned"){StopOwned(UInt32.Parse(args[1]));return;}
  if(args.Length==4&&args[0]=="lan-name"){LanName(UInt32.Parse(args[1]),new IntPtr(Convert.ToInt64(args[2],16)),args[3]);return;}
  if(args.Length==5&&args[0]=="serve-file"){ServeFile(args);return;}
  if(args.Length==5&&args[0]=="serve-game"){ServeGame(args);return;}
  if(args.Length>=5&&args[0]=="serve-ad"){ServeAdvertisement(args);return;}
  if(args.Length>2&&args[0]=="lan-mailbox"){Mailbox(args);return;}
  throw new Exception("Use launch-lan, stop-owned PID, lan-name PID HWND TEXT, serve-ad PID AD ENDPOINT TIMEOUT [ACCEPT], serve-file PID FILE ENDPOINT NAME, serve-game PID PEERS_TSV TIMEOUT_MS MAX_FRAME, lan-mailbox PID dump DIR|publish PACKET ENDPOINT CALLER");
 }
}
