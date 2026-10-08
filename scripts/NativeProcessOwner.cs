using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using Microsoft.Win32.SafeHandles;

namespace Numera.Launcher
{
    // The job is assigned atomically at creation, not after a runnable process exists.
    public sealed class NativeProcessOwner : IDisposable
    {
        private sealed class KernelHandle : SafeHandleZeroOrMinusOneIsInvalid
        {
            public KernelHandle(IntPtr value) : base(true) { SetHandle(value); }
            protected override bool ReleaseHandle() { return CloseHandle(handle); }
        }

        private KernelHandle job;
        private KernelHandle process;
        private KernelHandle thread;
        private bool emptyConfirmed;
        public int Id { get; private set; }
        public DateTime StartTimeUtc { get; private set; }

        public NativeProcessOwner(string executable, string[] arguments, string directory,
            IDictionary<string, string> environment, string outputPath, string errorPath)
        {
            if (!RuntimeInformation.IsOSPlatform(OSPlatform.Windows))
                throw new PlatformNotSupportedException("Owned launcher commands require Windows 10 or newer.");
            executable = Path.GetFullPath(executable);
            directory = Path.GetFullPath(directory);
            if (!File.Exists(executable) || !Directory.Exists(directory))
                throw new IOException("Owned command executable or working directory is unavailable.");
            var command = new StringBuilder(Quote(executable));
            foreach (string argument in arguments) command.Append(' ').Append(Quote(argument));
            if (command.Length >= 32767) throw new ArgumentException("Owned command line exceeds the Windows limit.");
            var environmentText = new StringBuilder();
            var keys = new List<string>(environment.Keys);
            keys.Sort(StringComparer.OrdinalIgnoreCase);
            foreach (string key in keys)
            {
                string value = environment[key] ?? "";
                if (key.Length == 0 || key.IndexOf('\0') >= 0 || key.IndexOf('=') >= 0 || value.IndexOf('\0') >= 0)
                    throw new ArgumentException("Owned command environment contains an invalid entry.");
                environmentText.Append(key).Append('=').Append(value).Append('\0');
            }
            environmentText.Append('\0');
            IntPtr attributes = IntPtr.Zero, handles = IntPtr.Zero, jobs = IntPtr.Zero, env = IntPtr.Zero;
            bool initialized = false;
            try
            {
                job = new KernelHandle(CreateJobObjectW(IntPtr.Zero, null));
                if (job.IsInvalid) Fail("Creating owned job failed");
                var limits = new ExtendedLimits();
                limits.Basic.LimitFlags = 0x2000; // KILL_ON_JOB_CLOSE; neither breakaway flag is enabled.
                if (!SetInformationJobObject(job, 9, ref limits, (uint)Marshal.SizeOf<ExtendedLimits>()))
                    Fail("Setting owned job limits failed");
                using (KernelHandle input = OpenInheritedFile("NUL", 0x80000000, 3))
                using (KernelHandle output = OpenInheritedFile(outputPath, 0x40000000, 1))
                using (KernelHandle error = OpenInheritedFile(errorPath, 0x40000000, 1))
                {
                    IntPtr size = IntPtr.Zero;
                    InitializeProcThreadAttributeList(IntPtr.Zero, 2, 0, ref size);
                    if (size == IntPtr.Zero) Fail("Sizing owned process attributes failed");
                    attributes = Marshal.AllocHGlobal(size);
                    if (!InitializeProcThreadAttributeList(attributes, 2, 0, ref size))
                        Fail("Initializing owned process attributes failed");
                    initialized = true;
                    handles = Marshal.AllocHGlobal(3 * IntPtr.Size);
                    Marshal.WriteIntPtr(handles, 0, input.DangerousGetHandle());
                    Marshal.WriteIntPtr(handles, IntPtr.Size, output.DangerousGetHandle());
                    Marshal.WriteIntPtr(handles, 2 * IntPtr.Size, error.DangerousGetHandle());
                    if (!UpdateProcThreadAttribute(attributes, 0, new IntPtr(0x20002), handles,
                        new IntPtr(3 * IntPtr.Size), IntPtr.Zero, IntPtr.Zero))
                        Fail("Setting owned standard handle list failed");
                    jobs = Marshal.AllocHGlobal(IntPtr.Size);
                    Marshal.WriteIntPtr(jobs, job.DangerousGetHandle());
                    if (!UpdateProcThreadAttribute(attributes, 0, new IntPtr(0x2000d), jobs,
                        new IntPtr(IntPtr.Size), IntPtr.Zero, IntPtr.Zero))
                        Fail("Setting atomic owned job list failed");
                    env = Marshal.StringToHGlobalUni(environmentText.ToString());
                    var startup = new StartupInfoEx();
                    startup.Startup.Size = (uint)Marshal.SizeOf<StartupInfoEx>();
                    startup.Startup.Flags = 0x101; // USESTDHANDLES | USESHOWWINDOW, SW_HIDE.
                    startup.Startup.Input = input.DangerousGetHandle();
                    startup.Startup.Output = output.DangerousGetHandle();
                    startup.Startup.Error = error.DangerousGetHandle();
                    startup.Attributes = attributes;
                    ProcessInformation created;
                    // SUSPENDED | UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT | NO_WINDOW.
                    if (!CreateProcessW(executable, command, IntPtr.Zero, IntPtr.Zero, true, 0x08080404,
                        env, directory, ref startup, out created)) Fail("Creating atomically owned process failed");
                    process = new KernelHandle(created.Process);
                    thread = new KernelHandle(created.Thread);
                    Id = checked((int)created.ProcessId);
                    long creation, exit, kernel, user;
                    if (!GetProcessTimes(process, out creation, out exit, out kernel, out user))
                        Fail("Reading owned process identity failed");
                    StartTimeUtc = DateTime.FromFileTimeUtc(creation);
                }
            }
            catch
            {
                try { if (process != null) StopAndConfirm(5000); }
                finally { Dispose(); }
                throw;
            }
            finally
            {
                if (initialized) DeleteProcThreadAttributeList(attributes);
                foreach (IntPtr value in new[] { attributes, handles, jobs, env })
                    if (value != IntPtr.Zero) Marshal.FreeHGlobal(value);
            }
        }

        private static KernelHandle OpenInheritedFile(string path, uint access, uint disposition)
        {
            var security = new SecurityAttributes { Length = (uint)Marshal.SizeOf<SecurityAttributes>(), Inherit = true };
            var handle = new KernelHandle(CreateFileW(path, access, 3, ref security, disposition, 0x80, IntPtr.Zero));
            if (handle.IsInvalid) { handle.Dispose(); Fail("Opening owned standard stream failed"); }
            return handle;
        }

        private static string Quote(string value)
        {
            if (value == null || value.IndexOf('\0') >= 0) throw new ArgumentException("Owned command argument is invalid.");
            var result = new StringBuilder("\"");
            int slashes = 0;
            foreach (char character in value)
            {
                if (character == '\\') { slashes++; continue; }
                result.Append('\\', character == '"' ? slashes * 2 + 1 : slashes);
                result.Append(character);
                slashes = 0;
            }
            return result.Append('\\', slashes * 2).Append('"').ToString();
        }

        public void Resume()
        {
            if (thread == null || thread.IsClosed) throw new InvalidOperationException("Owned root is already resumed.");
            if (ResumeThread(thread) == uint.MaxValue) Fail("Resuming owned root failed");
            thread.Dispose();
        }

        public bool WaitForExit(int milliseconds)
        {
            if (milliseconds < 0) throw new ArgumentOutOfRangeException(nameof(milliseconds));
            uint result = WaitForSingleObject(process, (uint)milliseconds);
            if (result == uint.MaxValue) Fail("Waiting for owned root failed");
            if (result != 0 && result != 258) throw new InvalidOperationException("Unexpected owned root wait state.");
            return result == 0;
        }

        public int ExitCode
        {
            get
            {
                if (!WaitForExit(0)) throw new InvalidOperationException("Owned root has not exited.");
                uint code;
                if (!GetExitCodeProcess(process, out code)) Fail("Reading owned root exit code failed");
                return unchecked((int)code);
            }
        }

        private Accounting Query()
        {
            Accounting accounting;
            if (!QueryInformationJobObject(job, 1, out accounting, (uint)Marshal.SizeOf<Accounting>(), IntPtr.Zero))
                Fail("Querying owned job state failed");
            return accounting;
        }
        public double CpuSeconds { get { var state = Query(); return (state.User + state.Kernel) / 10000000.0; } }

        public void StopAndConfirm(int milliseconds)
        {
            if (milliseconds <= 0) throw new ArgumentOutOfRangeException(nameof(milliseconds));
            if (emptyConfirmed) return;
            var watch = Stopwatch.StartNew();
            if (Query().ActiveProcesses != 0 && !TerminateJobObject(job, 1)) Fail("Terminating owned job failed");
            while (Query().ActiveProcesses != 0)
            {
                if (watch.ElapsedMilliseconds >= milliseconds)
                    throw new TimeoutException("Owned job did not reach zero active processes.");
                // Job handles are not signaled on ordinary emptiness; poll accounting under a deadline.
                Thread.Sleep(10);
            }
            if (process != null && !WaitForExit(Math.Max(0, milliseconds - (int)watch.ElapsedMilliseconds)))
                throw new TimeoutException("Owned root termination could not be confirmed.");
            emptyConfirmed = true;
        }

        public void Dispose()
        {
            // Closing the noninherited job handle remains the crash/cancellation safety net.
            if (job != null) job.Dispose();
            if (thread != null) thread.Dispose();
            if (process != null) process.Dispose();
        }
        private static void Fail(string message) { throw new Win32Exception(Marshal.GetLastWin32Error(), message); }

        [StructLayout(LayoutKind.Sequential)] private struct SecurityAttributes { public uint Length; public IntPtr Descriptor; [MarshalAs(UnmanagedType.Bool)] public bool Inherit; }
        [StructLayout(LayoutKind.Sequential)] private struct BasicLimits { public long ProcessTime, JobTime; public uint LimitFlags; public UIntPtr Minimum, Maximum; public uint ActiveLimit; public UIntPtr Affinity; public uint Priority, Scheduling; }
        [StructLayout(LayoutKind.Sequential)] private struct IoCounters { public ulong ReadCount, WriteCount, OtherCount, ReadBytes, WriteBytes, OtherBytes; }
        [StructLayout(LayoutKind.Sequential)] private struct ExtendedLimits { public BasicLimits Basic; public IoCounters Io; public UIntPtr ProcessMemory, JobMemory, PeakProcessMemory, PeakJobMemory; }
        [StructLayout(LayoutKind.Sequential)] private struct Accounting { public long User, Kernel, PeriodUser, PeriodKernel; public uint Faults, TotalProcesses, ActiveProcesses, TerminatedProcesses; }
        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] private struct StartupInfo
        {
            public uint Size; public string Reserved, Desktop, Title;
            public uint X, Y, XSize, YSize, XChars, YChars, Fill, Flags;
            public ushort Show, ReservedSize; public IntPtr ReservedData, Input, Output, Error;
        }
        [StructLayout(LayoutKind.Sequential)] private struct StartupInfoEx { public StartupInfo Startup; public IntPtr Attributes; }
        [StructLayout(LayoutKind.Sequential)] private struct ProcessInformation { public IntPtr Process, Thread; public uint ProcessId, ThreadId; }

        [DllImport("kernel32.dll", SetLastError = true)] private static extern bool CloseHandle(IntPtr handle);
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] private static extern IntPtr CreateJobObjectW(IntPtr attributes, string name);
        [DllImport("kernel32.dll", SetLastError = true)] private static extern bool SetInformationJobObject(KernelHandle job, int info, ref ExtendedLimits limits, uint size);
        [DllImport("kernel32.dll", SetLastError = true)] private static extern bool QueryInformationJobObject(KernelHandle job, int info, out Accounting state, uint size, IntPtr returned);
        [DllImport("kernel32.dll", SetLastError = true)] private static extern bool TerminateJobObject(KernelHandle job, uint code);
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] private static extern IntPtr CreateFileW(string name, uint access, uint share, ref SecurityAttributes attributes, uint disposition, uint flags, IntPtr template);
        [DllImport("kernel32.dll", SetLastError = true)] private static extern bool InitializeProcThreadAttributeList(IntPtr attributes, int count, uint flags, ref IntPtr size);
        [DllImport("kernel32.dll", SetLastError = true)] private static extern bool UpdateProcThreadAttribute(IntPtr attributes, uint flags, IntPtr key, IntPtr value, IntPtr size, IntPtr previous, IntPtr returned);
        [DllImport("kernel32.dll")] private static extern void DeleteProcThreadAttributeList(IntPtr attributes);
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] private static extern bool CreateProcessW(string application, StringBuilder command, IntPtr processAttributes, IntPtr threadAttributes, bool inherit, uint flags, IntPtr environment, string directory, ref StartupInfoEx startup, out ProcessInformation process);
        [DllImport("kernel32.dll", SetLastError = true)] private static extern uint ResumeThread(KernelHandle thread);
        [DllImport("kernel32.dll", SetLastError = true)] private static extern uint WaitForSingleObject(KernelHandle handle, uint milliseconds);
        [DllImport("kernel32.dll", SetLastError = true)] private static extern bool GetExitCodeProcess(KernelHandle process, out uint code);
        [DllImport("kernel32.dll", SetLastError = true)] private static extern bool GetProcessTimes(KernelHandle process, out long creation, out long exit, out long kernel, out long user);
    }
}
