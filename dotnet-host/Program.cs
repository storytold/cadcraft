using System;
using System.Collections;
using System.Collections.Generic;
using System.ComponentModel;
using System.Diagnostics;
using System.Drawing;
using System.Globalization;
using System.IO;
using System.Net.Sockets;
using System.Reflection;
using System.Resources;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Runtime.Remoting;
using System.Runtime.Remoting.Activation;
using System.Runtime.Remoting.Messaging;
using System.Runtime.Remoting.Proxies;
using System.Runtime.Versioning;
using System.Text;
using System.Threading;
using System.Web.Script.Serialization;
using System.Windows.Forms;
using Microsoft.Win32;

// Type forwarders so .NET 5/6/7/8 plugins (referencing System.Runtime, Microsoft.VisualBasic.Core,
// Microsoft.VisualBasic.Forms, System.Drawing.Common, System.Drawing.Primitives, etc.) seamlessly
// resolve to .NET Framework 4.8 runtime types when hosted by cadcraft-nethost.exe.
[assembly: TypeForwardedTo(typeof(object))]
[assembly: TypeForwardedTo(typeof(ValueType))]
[assembly: TypeForwardedTo(typeof(Enum))]
[assembly: TypeForwardedTo(typeof(Array))]
[assembly: TypeForwardedTo(typeof(Delegate))]
[assembly: TypeForwardedTo(typeof(MulticastDelegate))]
[assembly: TypeForwardedTo(typeof(Exception))]
[assembly: TypeForwardedTo(typeof(Attribute))]
[assembly: TypeForwardedTo(typeof(IDisposable))]
[assembly: TypeForwardedTo(typeof(IAsyncResult))]
[assembly: TypeForwardedTo(typeof(AsyncCallback))]
[assembly: TypeForwardedTo(typeof(EventHandler))]
[assembly: TypeForwardedTo(typeof(EventArgs))]
[assembly: TypeForwardedTo(typeof(String))]
[assembly: TypeForwardedTo(typeof(Boolean))]
[assembly: TypeForwardedTo(typeof(Byte))]
[assembly: TypeForwardedTo(typeof(Int16))]
[assembly: TypeForwardedTo(typeof(Int32))]
[assembly: TypeForwardedTo(typeof(Int64))]
[assembly: TypeForwardedTo(typeof(Single))]
[assembly: TypeForwardedTo(typeof(Double))]
[assembly: TypeForwardedTo(typeof(Decimal))]
[assembly: TypeForwardedTo(typeof(Char))]
[assembly: TypeForwardedTo(typeof(IntPtr))]
[assembly: TypeForwardedTo(typeof(UIntPtr))]
[assembly: TypeForwardedTo(typeof(Guid))]
[assembly: TypeForwardedTo(typeof(DateTime))]
[assembly: TypeForwardedTo(typeof(TimeSpan))]
[assembly: TypeForwardedTo(typeof(Math))]
[assembly: TypeForwardedTo(typeof(Convert))]
[assembly: TypeForwardedTo(typeof(Activator))]
[assembly: TypeForwardedTo(typeof(Type))]
[assembly: TypeForwardedTo(typeof(RuntimeTypeHandle))]
[assembly: TypeForwardedTo(typeof(WeakReference))]
[assembly: TypeForwardedTo(typeof(StringSplitOptions))]
[assembly: TypeForwardedTo(typeof(FileNotFoundException))]
[assembly: TypeForwardedTo(typeof(IOException))]
[assembly: TypeForwardedTo(typeof(ArgumentException))]
[assembly: TypeForwardedTo(typeof(ArgumentNullException))]
[assembly: TypeForwardedTo(typeof(InvalidOperationException))]
[assembly: TypeForwardedTo(typeof(NotSupportedException))]
[assembly: TypeForwardedTo(typeof(FormatException))]
[assembly: TypeForwardedTo(typeof(IndexOutOfRangeException))]
[assembly: TypeForwardedTo(typeof(NullReferenceException))]
[assembly: TypeForwardedTo(typeof(Container))]
[assembly: TypeForwardedTo(typeof(DateTimePicker))]
[assembly: TypeForwardedTo(typeof(DateTimePickerFormat))]
[assembly: TypeForwardedTo(typeof(NumericUpDown))]
[assembly: TypeForwardedTo(typeof(ListView))]
[assembly: TypeForwardedTo(typeof(ListViewItem))]
[assembly: TypeForwardedTo(typeof(ColumnHeader))]
[assembly: TypeForwardedTo(typeof(View))]
[assembly: TypeForwardedTo(typeof(SortOrder))]
[assembly: TypeForwardedTo(typeof(ItemActivation))]
[assembly: TypeForwardedTo(typeof(ColumnHeaderStyle))]
[assembly: TypeForwardedTo(typeof(RightToLeft))]
[assembly: TypeForwardedTo(typeof(AutoCompleteMode))]
[assembly: TypeForwardedTo(typeof(AutoCompleteSource))]
[assembly: TypeForwardedTo(typeof(AutoSizeMode))]
[assembly: TypeForwardedTo(typeof(IFormatProvider))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.MyServices.Internal.ContextValue<>))]
[assembly: TypeForwardedTo(typeof( STAThreadAttribute))]
[assembly: TypeForwardedTo(typeof(ThreadStaticAttribute))]
[assembly: TypeForwardedTo(typeof(ParamArrayAttribute))]
[assembly: TypeForwardedTo(typeof(ObsoleteAttribute))]
[assembly: TypeForwardedTo(typeof(FlagsAttribute))]
[assembly: TypeForwardedTo(typeof(AttributeUsageAttribute))]
[assembly: TypeForwardedTo(typeof(AttributeTargets))]
[assembly: TypeForwardedTo(typeof(IEnumerable))]
[assembly: TypeForwardedTo(typeof(IEnumerator))]
[assembly: TypeForwardedTo(typeof(ICollection))]
[assembly: TypeForwardedTo(typeof(IList))]
[assembly: TypeForwardedTo(typeof(Hashtable))]
[assembly: TypeForwardedTo(typeof(ArrayList))]
[assembly: TypeForwardedTo(typeof(List<>))]
[assembly: TypeForwardedTo(typeof(Dictionary<,>))]
[assembly: TypeForwardedTo(typeof(IEnumerable<>))]
[assembly: TypeForwardedTo(typeof(IEnumerator<>))]
[assembly: TypeForwardedTo(typeof(ICollection<>))]
[assembly: TypeForwardedTo(typeof(IList<>))]
[assembly: TypeForwardedTo(typeof(Stream))]
[assembly: TypeForwardedTo(typeof(FileStream))]
[assembly: TypeForwardedTo(typeof(MemoryStream))]
[assembly: TypeForwardedTo(typeof(StreamReader))]
[assembly: TypeForwardedTo(typeof(StreamWriter))]
[assembly: TypeForwardedTo(typeof(TextReader))]
[assembly: TypeForwardedTo(typeof(TextWriter))]
[assembly: TypeForwardedTo(typeof(File))]
[assembly: TypeForwardedTo(typeof(Directory))]
[assembly: TypeForwardedTo(typeof(Path))]
[assembly: TypeForwardedTo(typeof(Assembly))]
[assembly: TypeForwardedTo(typeof(AssemblyCompanyAttribute))]
[assembly: TypeForwardedTo(typeof(AssemblyCopyrightAttribute))]
[assembly: TypeForwardedTo(typeof(AssemblyDescriptionAttribute))]
[assembly: TypeForwardedTo(typeof(AssemblyFileVersionAttribute))]
[assembly: TypeForwardedTo(typeof(AssemblyInformationalVersionAttribute))]
[assembly: TypeForwardedTo(typeof(AssemblyProductAttribute))]
[assembly: TypeForwardedTo(typeof(AssemblyTitleAttribute))]
[assembly: TypeForwardedTo(typeof(AssemblyTrademarkAttribute))]
[assembly: TypeForwardedTo(typeof(DefaultMemberAttribute))]
[assembly: TypeForwardedTo(typeof(Missing))]
[assembly: TypeForwardedTo(typeof(TargetInvocationException))]
[assembly: TypeForwardedTo(typeof(ResourceManager))]
[assembly: TypeForwardedTo(typeof(CultureInfo))]
[assembly: TypeForwardedTo(typeof(StringBuilder))]
[assembly: TypeForwardedTo(typeof(Encoding))]
[assembly: TypeForwardedTo(typeof(Monitor))]
[assembly: TypeForwardedTo(typeof(Interlocked))]
[assembly: TypeForwardedTo(typeof(Thread))]
[assembly: TypeForwardedTo(typeof(CompilationRelaxationsAttribute))]
[assembly: TypeForwardedTo(typeof(RuntimeCompatibilityAttribute))]
[assembly: TypeForwardedTo(typeof(CompilerGeneratedAttribute))]
[assembly: TypeForwardedTo(typeof(AccessedThroughPropertyAttribute))]
[assembly: TypeForwardedTo(typeof(DecimalConstantAttribute))]
[assembly: TypeForwardedTo(typeof(DateTimeConstantAttribute))]
[assembly: TypeForwardedTo(typeof(MethodImplAttribute))]
[assembly: TypeForwardedTo(typeof(MethodImplOptions))]
[assembly: TypeForwardedTo(typeof(MethodCodeType))]
[assembly: TypeForwardedTo(typeof(RuntimeHelpers))]
[assembly: TypeForwardedTo(typeof(ComVisibleAttribute))]
[assembly: TypeForwardedTo(typeof(GuidAttribute))]
[assembly: TypeForwardedTo(typeof(Marshal))]
[assembly: TypeForwardedTo(typeof(ComImportAttribute))]
[assembly: TypeForwardedTo(typeof(CoClassAttribute))]
[assembly: TypeForwardedTo(typeof(DispIdAttribute))]
[assembly: TypeForwardedTo(typeof(InterfaceTypeAttribute))]
[assembly: TypeForwardedTo(typeof(ComInterfaceType))]
[assembly: TypeForwardedTo(typeof(TypeIdentifierAttribute))]
[assembly: TypeForwardedTo(typeof(DefaultCharSetAttribute))]
[assembly: TypeForwardedTo(typeof(CharSet))]
[assembly: TypeForwardedTo(typeof(OutAttribute))]
[assembly: TypeForwardedTo(typeof(InAttribute))]
[assembly: TypeForwardedTo(typeof(OptionalAttribute))]
[assembly: TypeForwardedTo(typeof(MarshalAsAttribute))]
[assembly: TypeForwardedTo(typeof(UnmanagedType))]
[assembly: TypeForwardedTo(typeof(TargetFrameworkAttribute))]
[assembly: TypeForwardedTo(typeof(DebuggableAttribute))]
[assembly: TypeForwardedTo(typeof(DebuggerBrowsableAttribute))]
[assembly: TypeForwardedTo(typeof(DebuggerBrowsableState))]
[assembly: TypeForwardedTo(typeof(DebuggerHiddenAttribute))]
[assembly: TypeForwardedTo(typeof(DebuggerNonUserCodeAttribute))]
[assembly: TypeForwardedTo(typeof(DebuggerStepThroughAttribute))]
[assembly: TypeForwardedTo(typeof(Process))]
[assembly: TypeForwardedTo(typeof(Component))]
[assembly: TypeForwardedTo(typeof(IContainer))]
[assembly: TypeForwardedTo(typeof(IComponent))]
[assembly: TypeForwardedTo(typeof(ISupportInitialize))]
[assembly: TypeForwardedTo(typeof(ComponentResourceManager))]
[assembly: TypeForwardedTo(typeof(DesignerCategoryAttribute))]
[assembly: TypeForwardedTo(typeof(EditorBrowsableAttribute))]
[assembly: TypeForwardedTo(typeof(EditorBrowsableState))]
[assembly: TypeForwardedTo(typeof(CancelEventArgs))]
[assembly: TypeForwardedTo(typeof(CancelEventHandler))]
[assembly: TypeForwardedTo(typeof(Color))]
[assembly: TypeForwardedTo(typeof(Font))]
[assembly: TypeForwardedTo(typeof(FontStyle))]
[assembly: TypeForwardedTo(typeof(GraphicsUnit))]
[assembly: TypeForwardedTo(typeof(Icon))]
[assembly: TypeForwardedTo(typeof(Image))]
[assembly: TypeForwardedTo(typeof(Bitmap))]
[assembly: TypeForwardedTo(typeof(Point))]
[assembly: TypeForwardedTo(typeof(PointF))]
[assembly: TypeForwardedTo(typeof(Size))]
[assembly: TypeForwardedTo(typeof(SizeF))]
[assembly: TypeForwardedTo(typeof(Rectangle))]
[assembly: TypeForwardedTo(typeof(ContentAlignment))]
[assembly: TypeForwardedTo(typeof(SystemColors))]
[assembly: TypeForwardedTo(typeof(Form))]
[assembly: TypeForwardedTo(typeof(Control))]
[assembly: TypeForwardedTo(typeof(ContainerControl))]
[assembly: TypeForwardedTo(typeof(ScrollableControl))]
[assembly: TypeForwardedTo(typeof(UserControl))]
[assembly: TypeForwardedTo(typeof(Button))]
[assembly: TypeForwardedTo(typeof(ButtonBase))]
[assembly: TypeForwardedTo(typeof(CheckBox))]
[assembly: TypeForwardedTo(typeof(RadioButton))]
[assembly: TypeForwardedTo(typeof(TextBox))]
[assembly: TypeForwardedTo(typeof(TextBoxBase))]
[assembly: TypeForwardedTo(typeof(Label))]
[assembly: TypeForwardedTo(typeof(ComboBox))]
[assembly: TypeForwardedTo(typeof(ListBox))]
[assembly: TypeForwardedTo(typeof(ListControl))]
[assembly: TypeForwardedTo(typeof(GroupBox))]
[assembly: TypeForwardedTo(typeof(Panel))]
[assembly: TypeForwardedTo(typeof(PictureBox))]
[assembly: TypeForwardedTo(typeof(TabControl))]
[assembly: TypeForwardedTo(typeof(TabPage))]
[assembly: TypeForwardedTo(typeof(ToolTip))]
[assembly: TypeForwardedTo(typeof(OpenFileDialog))]
[assembly: TypeForwardedTo(typeof(SaveFileDialog))]
[assembly: TypeForwardedTo(typeof(FileDialog))]
[assembly: TypeForwardedTo(typeof(CommonDialog))]
[assembly: TypeForwardedTo(typeof(MessageBox))]
[assembly: TypeForwardedTo(typeof(MessageBoxButtons))]
[assembly: TypeForwardedTo(typeof(MessageBoxIcon))]
[assembly: TypeForwardedTo(typeof(DialogResult))]
[assembly: TypeForwardedTo(typeof(AutoScaleMode))]
[assembly: TypeForwardedTo(typeof(FormBorderStyle))]
[assembly: TypeForwardedTo(typeof(FormStartPosition))]
[assembly: TypeForwardedTo(typeof(FormWindowState))]
[assembly: TypeForwardedTo(typeof(FormClosingEventArgs))]
[assembly: TypeForwardedTo(typeof(FormClosingEventHandler))]
[assembly: TypeForwardedTo(typeof(FormClosedEventArgs))]
[assembly: TypeForwardedTo(typeof(FormClosedEventHandler))]
[assembly: TypeForwardedTo(typeof(KeyPressEventArgs))]
[assembly: TypeForwardedTo(typeof(KeyPressEventHandler))]
[assembly: TypeForwardedTo(typeof(KeyEventArgs))]
[assembly: TypeForwardedTo(typeof(KeyEventHandler))]
[assembly: TypeForwardedTo(typeof(MouseEventArgs))]
[assembly: TypeForwardedTo(typeof(MouseEventHandler))]
[assembly: TypeForwardedTo(typeof(Cursors))]
[assembly: TypeForwardedTo(typeof(Cursor))]
[assembly: TypeForwardedTo(typeof(FlatStyle))]
[assembly: TypeForwardedTo(typeof(BorderStyle))]
[assembly: TypeForwardedTo(typeof(DockStyle))]
[assembly: TypeForwardedTo(typeof(AnchorStyles))]
[assembly: TypeForwardedTo(typeof(Padding))]
[assembly: TypeForwardedTo(typeof(ImeMode))]
[assembly: TypeForwardedTo(typeof(ComboBoxStyle))]
[assembly: TypeForwardedTo(typeof(HorizontalAlignment))]
[assembly: TypeForwardedTo(typeof( Microsoft.VisualBasic.Information))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.Interaction))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.Strings))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.Conversion))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.FileSystem))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.DateAndTime))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.VBMath))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.Financial))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.Globals))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.Constants))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.ErrObject))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.MsgBoxResult))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.MsgBoxStyle))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.CompareMethod))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.TriState))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.FirstDayOfWeek))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.FirstWeekOfYear))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.DateInterval))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.VariantType))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.AppWinStyle))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.CallType))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.OpenMode))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.OpenAccess))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.OpenShare))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.TabInfo))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.SpcInfo))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.HideModuleNameAttribute))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.MyGroupCollectionAttribute))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.ComClassAttribute))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.VBFixedArrayAttribute))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.VBFixedStringAttribute))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.CompilerServices.Conversions))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.CompilerServices.Operators))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.CompilerServices.NewLateBinding))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.CompilerServices.LateBinding))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.CompilerServices.ProjectData))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.CompilerServices.StandardModuleAttribute))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.CompilerServices.DesignerGeneratedAttribute))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.CompilerServices.OptionTextAttribute))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.CompilerServices.OptionCompareAttribute))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.CompilerServices.Utils))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.CompilerServices.Versioned))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.CompilerServices.LikeOperator))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.CompilerServices.ObjectFlowControl))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.CompilerServices.IncompleteInitialization))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.CompilerServices.StaticLocalInitFlag))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.ApplicationServices.ApplicationBase))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.ApplicationServices.WindowsFormsApplicationBase))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.ApplicationServices.ConsoleApplicationBase))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.ApplicationServices.AssemblyInfo))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.ApplicationServices.User))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.Devices.Computer))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.Devices.ServerComputer))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.MyServices.FileSystemProxy))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.MyServices.RegistryProxy))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.MyServices.ClipboardProxy))]
[assembly: TypeForwardedTo(typeof(Microsoft.VisualBasic.MyServices.SpecialDirectoriesProxy))]

namespace CadCraft.NetHost
{
    public static class BridgeClient
    {
        public static int ControlPort = 0;
        public static string CurrentDllDir = "";
        private static int _reqId = 1;
        private static readonly JavaScriptSerializer Json = new JavaScriptSerializer { MaxJsonLength = int.MaxValue };

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        public static extern bool DeleteFileW(string lpFileName);

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public static void UnblockFile(string path)
        {
            try
            {
                if (!string.IsNullOrEmpty(path))
                {
                    DeleteFileW(path + ":Zone.Identifier");
                }
            }
            catch { }
        }

        private static string _cachedToken = null;
        private static int _cachedTokenPort = 0;

        /// <summary>Read the per-session loopback control token issued by CADCraft for <see cref="ControlPort"/>.</summary>
        private static string ReadControlToken()
        {
            if (ControlPort <= 0) return "";
            if (_cachedToken != null && _cachedTokenPort == ControlPort) return _cachedToken;
            try
            {
                string envTok = Environment.GetEnvironmentVariable("CADCRAFT_CONTROL_TOKEN");
                if (!string.IsNullOrEmpty(envTok))
                {
                    _cachedToken = envTok.Trim();
                    _cachedTokenPort = ControlPort;
                    return _cachedToken;
                }
                string localAppData = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
                if (!string.IsNullOrEmpty(localAppData))
                {
                    string tokenFile = Path.Combine(localAppData, "cadcraft-control-" + ControlPort + ".token");
                    if (File.Exists(tokenFile))
                    {
                        _cachedToken = File.ReadAllText(tokenFile, Encoding.UTF8).Trim();
                        _cachedTokenPort = ControlPort;
                        return _cachedToken;
                    }
                }
            }
            catch { }
            return "";
        }

        /// <summary>Send a single JSON-line control request to CADCraft and return its parsed result object.</summary>
        public static Dictionary<string, object> Call(string method, object cmdParams)
        {
            if (ControlPort <= 0)
            {
                return new Dictionary<string, object>();
            }
            try
            {
                using (var client = new TcpClient("127.0.0.1", ControlPort))
                {
                    client.ReceiveTimeout = 60000;
                    client.SendTimeout = 15000;
                    using (var stream = client.GetStream())
                    using (var writer = new StreamWriter(stream, new UTF8Encoding(false)) { AutoFlush = true })
                    using (var reader = new StreamReader(stream, Encoding.UTF8))
                    {
                        var req = new Dictionary<string, object>
                        {
                            { "id", Interlocked.Increment(ref _reqId) },
                            { "method", method },
                            { "params", cmdParams ?? new Dictionary<string, object>() }
                        };
                        string tok = ReadControlToken();
                        if (!string.IsNullOrEmpty(tok))
                        {
                            req["token"] = tok;
                        }
                        writer.WriteLine(Json.Serialize(req));
                        string line = reader.ReadLine();
                        if (string.IsNullOrEmpty(line))
                        {
                            return new Dictionary<string, object>();
                        }
                        var resp = Json.Deserialize<Dictionary<string, object>>(line);
                        if (resp != null && resp.ContainsKey("result") && resp["result"] is Dictionary<string, object>)
                        {
                            return (Dictionary<string, object>)resp["result"];
                        }
                        return resp ?? new Dictionary<string, object>();
                    }
                }
            }
            catch
            {
                return new Dictionary<string, object>();
            }
        }

        /// <summary>Invoke a <c>dotnet.bridge</c> command operation in the CADCraft engine.</summary>
        public static Dictionary<string, object> Bridge(string op, Dictionary<string, object> args = null)
        {
            var p = args != null ? new Dictionary<string, object>(args) : new Dictionary<string, object>();
            p["op"] = op;
            if (!p.ContainsKey("dllDir") && !string.IsNullOrEmpty(CurrentDllDir))
            {
                p["dllDir"] = CurrentDllDir;
            }
            return Call("command", new Dictionary<string, object>
            {
                { "id", "dotnet.bridge" },
                { "params", p }
            });
        }

        /// <summary>Start an interactive pick in CADCraft and poll until the user completes or cancels it.</summary>
        public static Dictionary<string, object> InteractivePick(string mode, string prompt, double[] basePt = null)
        {
            var args = new Dictionary<string, object>
            {
                { "mode", mode },
                { "prompt", prompt ?? "Select in CADCraft" }
            };
            if (basePt != null && basePt.Length >= 2)
            {
                args["base"] = new object[] { basePt[0], basePt[1], basePt.Length > 2 ? basePt[2] : 0.0 };
            }
            Bridge("begin_pick", args);
            int failures = 0;
            while (true)
            {
                System.Windows.Forms.Application.DoEvents();
                Thread.Sleep(40);
                var st = Bridge("poll_pick");
                if (st == null || st.Count == 0)
                {
                    if (++failures > 5)
                    {
                        return new Dictionary<string, object> { { "status", "cancelled" } };
                    }
                    continue;
                }
                failures = 0;
                string status = GetStr(st, "status", "idle");
                if (status == "done" || status == "ok" || status == "cancelled" || status == "cancel")
                {
                    if (status == "ok") st["status"] = "done";
                    if (status == "cancel") st["status"] = "cancelled";
                    return st;
                }
            }
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public static string GetStr(Dictionary<string, object> d, string key, string def = "")
        {
            if (d != null && d.ContainsKey(key) && d[key] != null)
            {
                return Convert.ToString(d[key], CultureInfo.InvariantCulture);
            }
            return def;
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public static int GetInt(Dictionary<string, object> d, string key, int def = 0)
        {
            if (d != null && d.ContainsKey(key) && d[key] != null)
            {
                try { return Convert.ToInt32(d[key], CultureInfo.InvariantCulture); } catch { }
            }
            return def;
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public static double GetDbl(Dictionary<string, object> d, string key, double def = 0.0)
        {
            if (d != null && d.ContainsKey(key) && d[key] != null)
            {
                try { return Convert.ToDouble(d[key], CultureInfo.InvariantCulture); } catch { }
            }
            return def;
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public static bool GetBool(Dictionary<string, object> d, string key, bool def = false)
        {
            if (d != null && d.ContainsKey(key) && d[key] != null)
            {
                try { return Convert.ToBoolean(d[key], CultureInfo.InvariantCulture); } catch { }
            }
            return def;
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public static double[] GetDblArray(Dictionary<string, object> d, string key, double[] def = null)
        {
            if (d != null && d.ContainsKey(key) && d[key] is IEnumerable && !(d[key] is string))
            {
                var list = new List<double>();
                foreach (var item in (IEnumerable)d[key])
                {
                    try { list.Add(Convert.ToDouble(item, CultureInfo.InvariantCulture)); } catch { list.Add(0.0); }
                }
                return list.ToArray();
            }
            return def ?? new double[] { 0.0, 0.0, 0.0 };
        }
    }

    public class AcadDynamicTarget : MarshalByRefObject
    {
        public string Handle = "";
        public string ObjectName = "AcDbEntity";
        public string Name = "";
        public string Layer = "0";
        public int ColorIndex = 256;
        public int Lineweight = -1;
        public bool Closed = false;
        public double ConstantWidth = 0.0;
        public double Rotation = 0.0;
        public string TextString = "";
        public double[] Coordinates = new double[0];
        public double[] StartPoint = new double[] { 0.0, 0.0, 0.0 };
        public double[] EndPoint = new double[] { 0.0, 0.0, 0.0 };
        public double[] InsertionPoint = new double[] { 0.0, 0.0, 0.0 };
        public double[] MinBound = new double[] { 0.0, 0.0, 0.0 };
        public double[] MaxBound = new double[] { 10.0, 10.0, 0.0 };
        public List<Dictionary<string, object>> XData = new List<Dictionary<string, object>>();

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public double[] get_Coordinate(int index)
        {
            bool is3d = string.Equals(ObjectName, "AcDb3dPolyline", StringComparison.OrdinalIgnoreCase);
            int stride = is3d ? 3 : 2;
            int i = index * stride;
            if (Coordinates != null && i + stride - 1 < Coordinates.Length)
            {
                if (is3d)
                {
                    return new double[] { Coordinates[i], Coordinates[i + 1], Coordinates[i + 2] };
                }
                return new double[] { Coordinates[i], Coordinates[i + 1] };
            }
            return is3d ? new double[] { 0.0, 0.0, 0.0 } : new double[] { 0.0, 0.0 };
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public void set_Coordinate(int index, object val)
        {
            var pt = ToDblArray(val);
            bool is3d = string.Equals(ObjectName, "AcDb3dPolyline", StringComparison.OrdinalIgnoreCase);
            int stride = is3d ? 3 : 2;
            int i = index * stride;
            if (Coordinates != null && i + stride - 1 < Coordinates.Length && pt.Length >= 2)
            {
                Coordinates[i] = pt[0];
                Coordinates[i + 1] = pt[1];
                if (is3d && pt.Length >= 3)
                {
                    Coordinates[i + 2] = pt[2];
                }
                if (!string.IsNullOrEmpty(Handle))
                {
                    BridgeClient.Bridge("update_entity", new Dictionary<string, object>
                    {
                        { "handle", Handle },
                        { "coordinates", Coordinates }
                    });
                }
            }
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public static double[] ToDblArray(object val)
        {
            if (val is double[]) return (double[])val;
            if (val is IEnumerable && !(val is string))
            {
                var list = new List<double>();
                foreach (var x in (IEnumerable)val)
                {
                    try { list.Add(Convert.ToDouble(x, CultureInfo.InvariantCulture)); } catch { list.Add(0.0); }
                }
                return list.ToArray();
            }
            return new double[] { 0.0, 0.0, 0.0 };
        }
    }

    public abstract class AcadProxyBase : RealProxy, IRemotingTypeInfo
    {
        protected readonly AcadDynamicTarget Target;

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        protected AcadProxyBase(AcadDynamicTarget target = null) : base(typeof(AcadDynamicTarget))
        {
            Target = target ?? new AcadDynamicTarget();
        }

        public string TypeName
        {
            get { return "AcadDynamicTarget"; }
            set { }
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public bool CanCastTo(Type fromType, object o)
        {
            return true;
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public override IMessage Invoke(IMessage msg)
        {
            var call = msg as IMethodCallMessage;
            if (call == null)
            {
                return new ReturnMessage(null, null, 0, null, null);
            }
            object[] args = call.Args ?? new object[0];
            string m = call.MethodName ?? "";
            try
            {
                if (m == "GetType")
                {
                    return new ReturnMessage(typeof(AcadDynamicTarget), args, args.Length, call.LogicalCallContext, call);
                }
                if (m == "ToString")
                {
                    return new ReturnMessage(Target.ObjectName ?? "AcadObject", args, args.Length, call.LogicalCallContext, call);
                }
                if (m == "Equals")
                {
                    bool eq = args.Length > 0 && ReferenceEquals(GetTransparentProxy(), args[0]);
                    return new ReturnMessage(eq, args, args.Length, call.LogicalCallContext, call);
                }
                if (m == "GetHashCode")
                {
                    return new ReturnMessage(Target.GetHashCode(), args, args.Length, call.LogicalCallContext, call);
                }
                if (m == "FieldGetter" && args.Length >= 2)
                {
                    string fName = Convert.ToString(args[1]);
                    object val = HandleMember( "get_" + fName, new object[0]);
                    args[2] = val;
                    return new ReturnMessage(null, args, args.Length, call.LogicalCallContext, call);
                }
                if (m == "FieldSetter" && args.Length >= 3)
                {
                    string fName = Convert.ToString(args[1]);
                    HandleMember("set_" + fName, new object[] { args[2] });
                    return new ReturnMessage(null, args, args.Length, call.LogicalCallContext, call);
                }
                object result = HandleMember(m, args);
                return new ReturnMessage(result, args, args.Length, call.LogicalCallContext, call);
            }
            catch (Exception ex)
            {
                return new ReturnMessage(ex, call);
            }
        }

        protected abstract object HandleMember(string name, object[] args);
    }

    [AttributeUsage(AttributeTargets.Class)]
    public class AcadColorProxyAttr : ProxyAttribute
    {
        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public override MarshalByRefObject CreateInstance(Type serverType)
        {
            return (MarshalByRefObject)new AcadColorProxy().GetTransparentProxy();
        }
    }

    [ComVisible(true)]
    [Guid("D97EB149-1A3D-46FD-813B-59EFCE6E46B9")]
    [ClassInterface(ClassInterfaceType.None)]
    [AcadColorProxyAttr]
    public class AcadColorComServer : ContextBoundObject
    {
        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public AcadColorComServer() { }
    }

    public class AcadColorProxy : AcadProxyBase
    {
        public int ColorIndex = 256;
        public int ColorMethod = 195;

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public AcadColorProxy(int colorIndex = 256)
        {
            ColorIndex = colorIndex;
            Target.ColorIndex = colorIndex;
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        protected override object HandleMember(string name, object[] args)
        {
            switch (name)
            {
                case "get_ColorIndex":
                    return ColorIndex;
                case "set_ColorIndex":
                    if (args.Length > 0)
                    {
                        try { ColorIndex = Convert.ToInt32(args[0]); Target.ColorIndex = ColorIndex; } catch { }
                    }
                    return null;
                case "get_ColorMethod":
                    return ColorMethod;
                case "set_ColorMethod":
                    if (args.Length > 0)
                    {
                        try { ColorMethod = Convert.ToInt32(args[0]); } catch { }
                    }
                    return null;
                case "SetRGB":
                    return null;
                default:
                    return null;
            }
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public static void EnsureComRegistered()
        {
            try
            {
                string clsid = "{D97EB149-1A3D-46FD-813B-59EFCE6E46B9}";
                string keyPath = @"Software\Classes\CLSID\" + clsid + @"\InprocServer32";
                using (RegistryKey key = Registry.CurrentUser.CreateSubKey(keyPath))
                {
                    if (key != null)
                    {
                        key.SetValue("", "mscoree.dll");
                        key.SetValue("ThreadingModel", "Both");
                        key.SetValue("Class", typeof(AcadColorComServer).FullName);
                        key.SetValue("Assembly", typeof(AcadColorComServer).Assembly.FullName);
                        key.SetValue("CodeBase", typeof(AcadColorComServer).Assembly.Location);
                    }
                }
            }
            catch { }
        }
    }

    public class AcadEntityProxy : AcadProxyBase
    {
        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public AcadEntityProxy(AcadDynamicTarget t) : base(t) { }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public static object FromJson(Dictionary<string, object> d)
        {
            if (d == null || d.Count == 0 || !d.ContainsKey("handle"))
            {
                return null;
            }
            var t = new AcadDynamicTarget
            {
                Handle = BridgeClient.GetStr(d, "handle", ""),
                ObjectName = BridgeClient.GetStr(d, "objectName", "AcDbEntity"),
                Layer = BridgeClient.GetStr(d, "layer", "0"),
                ColorIndex = BridgeClient.GetInt(d, "color", 256),
                Lineweight = BridgeClient.GetInt(d, "lineweight", -1),
                Closed = BridgeClient.GetBool(d, "closed", false),
                ConstantWidth = BridgeClient.GetDbl(d, "constantWidth", 0.0),
                Rotation = BridgeClient.GetDbl(d, "rotation", 0.0),
                Name = BridgeClient.GetStr(d, "name", ""),
                TextString = BridgeClient.GetStr(d, "text", ""),
                Coordinates = BridgeClient.GetDblArray(d, "coordinates", new double[0]),
                StartPoint = BridgeClient.GetDblArray(d, "startPoint", new double[] { 0.0, 0.0, 0.0 }),
                EndPoint = BridgeClient.GetDblArray(d, "endPoint", new double[] { 0.0, 0.0, 0.0 }),
                InsertionPoint = BridgeClient.GetDblArray(d, "insertionPoint", new double[] { 0.0, 0.0, 0.0 })
            };
            if (d.ContainsKey("bounds") && d["bounds"] is IEnumerable)
            {
                var bList = new List<double[]>();
                foreach (var ptObj in (IEnumerable)d["bounds"])
                {
                    bList.Add(AcadDynamicTarget.ToDblArray(ptObj));
                }
                if (bList.Count >= 2)
                {
                    t.MinBound = bList[0];
                    t.MaxBound = bList[1];
                }
            }
            if (d.ContainsKey("xdata") && d["xdata"] is IEnumerable)
            {
                foreach (var item in (IEnumerable)d["xdata"])
                {
                    if (item is Dictionary<string, object>)
                    {
                        t.XData.Add((Dictionary<string, object>)item);
                    }
                }
            }
            return new AcadEntityProxy(t).GetTransparentProxy();
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        private void RefreshFromServer()
        {
            if (string.IsNullOrEmpty(Target.Handle)) return;
            var d = BridgeClient.Bridge("entity", new Dictionary<string, object> { { "handle", Target.Handle } });
            if (d != null && d.ContainsKey("handle"))
            {
                Target.Layer = BridgeClient.GetStr(d, "layer", Target.Layer);
                Target.ColorIndex = BridgeClient.GetInt(d, "color", Target.ColorIndex);
                Target.Lineweight = BridgeClient.GetInt(d, "lineweight", Target.Lineweight);
                Target.Closed = BridgeClient.GetBool(d, "closed", Target.Closed);
                Target.ConstantWidth = BridgeClient.GetDbl(d, "constantWidth", Target.ConstantWidth);
                Target.Rotation = BridgeClient.GetDbl(d, "rotation", Target.Rotation);
                Target.Name = BridgeClient.GetStr(d, "name", Target.Name);
                Target.TextString = BridgeClient.GetStr(d, "text", Target.TextString);
                Target.Coordinates = BridgeClient.GetDblArray(d, "coordinates", Target.Coordinates);
                Target.StartPoint = BridgeClient.GetDblArray(d, "startPoint", Target.StartPoint);
                Target.EndPoint = BridgeClient.GetDblArray(d, "endPoint", Target.EndPoint);
                Target.InsertionPoint = BridgeClient.GetDblArray(d, "insertionPoint", Target.InsertionPoint);
                if (d.ContainsKey("xdata") && d["xdata"] is IEnumerable)
                {
                    Target.XData.Clear();
                    foreach (var item in (IEnumerable)d["xdata"])
                    {
                        if (item is Dictionary<string, object>)
                        {
                            Target.XData.Add((Dictionary<string, object>)item);
                        }
                    }
                }
            }
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        protected override object HandleMember(string name, object[] args)
        {
            switch (name)
            {
                case "get_Handle":
                    return Target.Handle;
                case "get_ObjectName":
                case "get_EntityName":
                    return Target.ObjectName;
                case "get_Layer":
                    return Target.Layer;
                case "set_Layer":
                    if (args.Length > 0)
                    {
                        Target.Layer = Convert.ToString(args[0]);
                        BridgeClient.Bridge("update_entity", new Dictionary<string, object>
                        {
                            { "handle", Target.Handle },
                            { "layer", Target.Layer }
                        });
                    }
                    return null;
                case "get_TrueColor":
                    return new AcadColorProxy(Target.ColorIndex).GetTransparentProxy();
                case "set_TrueColor":
                    if (args.Length > 0 && args[0] != null)
                    {
                        int ci = Target.ColorIndex;
                        var rp = RemotingServices.GetRealProxy(args[0]) as AcadColorProxy;
                        if (rp != null) ci = rp.ColorIndex;
                        Target.ColorIndex = ci;
                        BridgeClient.Bridge("update_entity", new Dictionary<string, object>
                        {
                            { "handle", Target.Handle },
                            { "color", ci }
                        });
                    }
                    return null;
                case "get_color":
                case "get_Color":
                    return Target.ColorIndex;
                case "set_color":
                case "set_Color":
                    if (args.Length > 0)
                    {
                        try
                        {
                            Target.ColorIndex = Convert.ToInt32(args[0]);
                            BridgeClient.Bridge("update_entity", new Dictionary<string, object>
                            {
                                { "handle", Target.Handle },
                                { "color", Target.ColorIndex }
                            });
                        }
                        catch { }
                    }
                    return null;
                case "get_Lineweight":
                    return Target.Lineweight;
                case "set_Lineweight":
                    if (args.Length > 0)
                    {
                        try
                        {
                            Target.Lineweight = Convert.ToInt32(args[0]);
                            BridgeClient.Bridge("update_entity", new Dictionary<string, object>
                            {
                                { "handle", Target.Handle },
                                { "lineweight", Target.Lineweight }
                            });
                        }
                        catch { }
                    }
                    return null;
                case "get_Closed":
                case "get_closed":
                    return Target.Closed;
                case "set_Closed":
                case "set_closed":
                    if (args.Length > 0)
                    {
                        Target.Closed = Convert.ToBoolean(args[0]);
                        BridgeClient.Bridge("update_entity", new Dictionary<string, object>
                        {
                            { "handle", Target.Handle },
                            { "closed", Target.Closed }
                        });
                    }
                    return null;
                case "get_ConstantWidth":
                    return Target.ConstantWidth;
                case "set_ConstantWidth":
                    if (args.Length > 0)
                    {
                        Target.ConstantWidth = Convert.ToDouble(args[0], CultureInfo.InvariantCulture);
                        BridgeClient.Bridge("update_entity", new Dictionary<string, object>
                        {
                            { "handle", Target.Handle },
                            { "constantWidth", Target.ConstantWidth }
                        });
                    }
                    return null;
                case "get_Coordinates":
                    return Target.Coordinates;
                case "set_Coordinates":
                    if (args.Length > 0)
                    {
                        Target.Coordinates = AcadDynamicTarget.ToDblArray(args[0]);
                        BridgeClient.Bridge("update_entity", new Dictionary<string, object>
                        {
                            { "handle", Target.Handle },
                            { "coordinates", Target.Coordinates }
                        });
                    }
                    return null;
                case "get_Coordinate":
                    if (args.Length > 0)
                    {
                        return Target.get_Coordinate(Convert.ToInt32(args[0]));
                    }
                    return new double[] { 0.0, 0.0 };
                case "set_Coordinate":
                    if (args.Length >= 2)
                    {
                        Target.set_Coordinate(Convert.ToInt32(args[0]), args[1]);
                    }
                    return null;
                case "get_StartPoint":
                    return Target.StartPoint;
                case "get_EndPoint":
                    return Target.EndPoint;
                case "get_InsertionPoint":
                    return Target.InsertionPoint;
                case "get_Rotation":
                    return Target.Rotation;
                case "set_Rotation":
                    if (args.Length > 0)
                    {
                        Target.Rotation = Convert.ToDouble(args[0], CultureInfo.InvariantCulture);
                        BridgeClient.Bridge("update_entity", new Dictionary<string, object>
                        {
                            { "handle", Target.Handle },
                            { "rotation", Target.Rotation }
                        });
                    }
                    return null;
                case "get_Name":
                case "get_EffectiveName":
                    return Target.Name;
                case "set_Name":
                    if (args.Length > 0)
                    {
                        Target.Name = Convert.ToString(args[0]);
                        BridgeClient.Bridge("update_entity", new Dictionary<string, object>
                        {
                            { "handle", Target.Handle },
                            { "name", Target.Name }
                        });
                    }
                    return null;
                case "get_Alignment":
                    return 0;
                case "set_Alignment":
                    if (args.Length > 0)
                    {
                        BridgeClient.Bridge("update_entity", new Dictionary<string, object>
                        {
                            { "handle", Target.Handle },
                            { "alignment", Convert.ToInt32(args[0]) }
                        });
                    }
                    return null;
                case "get_TextAlignmentPoint":
                    return Target.InsertionPoint;
                case "set_TextAlignmentPoint":
                    if (args.Length > 0)
                    {
                        var pt = AcadDynamicTarget.ToDblArray(args[0]);
                        Target.InsertionPoint = pt;
                        BridgeClient.Bridge("update_entity", new Dictionary<string, object>
                        {
                            { "handle", Target.Handle },
                            { "textAlignmentPoint", pt }
                        });
                    }
                    return null;
                case "GetBoundingBox":
                    if (args.Length >= 2)
                    {
                        args[0] = Target.MinBound;
                        args[1] = Target.MaxBound;
                    }
                    return null;
                case "Highlight":
                    if (args.Length > 0)
                    {
                        BridgeClient.Bridge("highlight", new Dictionary<string, object>
                        {
                            { "handle", Target.Handle },
                            { "on", Convert.ToBoolean(args[0]) }
                        });
                    }
                    return null;
                case "Update":
                    RefreshFromServer();
                    return null;
                case "Delete":
                    BridgeClient.Bridge("delete_entity", new Dictionary<string, object> { { "handle", Target.Handle } });
                    return null;
                case "Rotate":
                    if (args.Length >= 2)
                    {
                        var bp = AcadDynamicTarget.ToDblArray(args[0]);
                        double ang = Convert.ToDouble(args[1], CultureInfo.InvariantCulture);
                        BridgeClient.Bridge("update_entity", new Dictionary<string, object>
                        {
                            { "handle", Target.Handle },
                            { "rotateBase", bp },
                            { "rotateAngle", ang }
                        });
                        RefreshFromServer();
                    }
                    return null;
                case "Offset":
                    if (args.Length >= 1)
                    {
                        double dist = Convert.ToDouble(args[0], CultureInfo.InvariantCulture);
                        var res = BridgeClient.Bridge("offset_lwpolyline", new Dictionary<string, object>
                        {
                            { "handle", Target.Handle },
                            { "distance", dist }
                        });
                        var list = new List<object>();
                        if (res != null && res.ContainsKey("entities") && res["entities"] is IEnumerable)
                        {
                            foreach (var item in (IEnumerable)res["entities"])
                            {
                                if (item is Dictionary<string, object>)
                                {
                                    var ent = FromJson((Dictionary<string, object>)item);
                                    if (ent != null) list.Add(ent);
                                }
                            }
                        }
                        return list.ToArray();
                    }
                    return new object[0];
                case "IntersectWith":
                    if (args.Length >= 1 && args[0] != null)
                    {
                        var otherProxy = RemotingServices.GetRealProxy(args[0]) as AcadEntityProxy;
                        if (otherProxy != null)
                        {
                            return ComputeIntersections(Target, otherProxy.Target);
                        }
                    }
                    return new double[0];
                case "GetXData":
                    {
                        RefreshFromServer();
                        string appFilter = args.Length > 0 && args[0] != null ? Convert.ToString(args[0]).Trim() : "";
                        var filtered = new List<Dictionary<string, object>>();
                        bool include = string.IsNullOrEmpty(appFilter) || appFilter == "*";
                        foreach (var tag in Target.XData)
                        {
                            int code = BridgeClient.GetInt(tag, "code", 1000);
                            string val = BridgeClient.GetStr(tag, "value", "");
                            if (code == 1001)
                            {
                                include = string.IsNullOrEmpty(appFilter) || appFilter == "*" || string.Equals(val, appFilter, StringComparison.OrdinalIgnoreCase);
                            }
                            if (include)
                            {
                                filtered.Add(tag);
                            }
                        }
                        if (args.Length >= 3)
                        {
                            if (filtered.Count == 0)
                            {
                                args[1] = null;
                                args[2] = null;
                            }
                            else
                            {
                                short[] codes = new short[filtered.Count];
                                object[] vals = new object[filtered.Count];
                                for (int i = 0; i < filtered.Count; i++)
                                {
                                    codes[i] = (short)BridgeClient.GetInt(filtered[i], "code", 1000);
                                    vals[i] = BridgeClient.GetStr(filtered[i], "value", "");
                                }
                                args[1] = codes;
                                args[2] = vals;
                            }
                        }
                        return null;
                    }
                case "SetXData":
                    if (args.Length >= 2 && args[0] is IEnumerable && args[1] is IEnumerable)
                    {
                        var codes = new List<int>();
                        foreach (var c in (IEnumerable)args[0]) codes.Add(Convert.ToInt32(c));
                        var vals = new List<string>();
                        foreach (var v in (IEnumerable)args[1]) vals.Add(v != null ? Convert.ToString(v, CultureInfo.InvariantCulture) : "");
                        int count = Math.Min(codes.Count, vals.Count);
                        var xdataPayload = new List<Dictionary<string, object>>();
                        for (int i = 0; i < count; i++)
                        {
                            xdataPayload.Add(new Dictionary<string, object>
                            {
                                { "code", codes[i] },
                                { "value", vals[i] }
                            });
                        }
                        BridgeClient.Bridge("set_xdata", new Dictionary<string, object>
                        {
                            { "handle", Target.Handle },
                            { "xdata", xdataPayload }
                        });
                        RefreshFromServer();
                    }
                    return null;
                default:
                    return null;
            }
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        private static List<double[]> ExtractVertices3D(AcadDynamicTarget t)
        {
            var pts = new List<double[]>();
            if (t == null) return pts;
            if (string.Equals(t.ObjectName, "AcDbLine", StringComparison.OrdinalIgnoreCase))
            {
                pts.Add(new double[] { t.StartPoint[0], t.StartPoint[1], t.StartPoint.Length > 2 ? t.StartPoint[2] : 0.0 });
                pts.Add(new double[] { t.EndPoint[0], t.EndPoint[1], t.EndPoint.Length > 2 ? t.EndPoint[2] : 0.0 });
            }
            else if (string.Equals(t.ObjectName, "AcDb3dPolyline", StringComparison.OrdinalIgnoreCase))
            {
                for (int i = 0; i + 2 < t.Coordinates.Length; i += 3)
                {
                    pts.Add(new double[] { t.Coordinates[i], t.Coordinates[i + 1], t.Coordinates[i + 2] });
                }
            }
            else
            {
                for (int i = 0; i + 1 < t.Coordinates.Length; i += 2)
                {
                    pts.Add(new double[] { t.Coordinates[i], t.Coordinates[i + 1], 0.0 });
                }
            }
            return pts;
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        private static double[] ComputeIntersections(AcadDynamicTarget a, AcadDynamicTarget b)
        {
            var va = ExtractVertices3D(a);
            var vb = ExtractVertices3D(b);
            var hits = new List<double>();
            for (int i = 0; i + 1 < va.Count; i++)
            {
                double x1 = va[i][0], y1 = va[i][1], z1 = va[i][2];
                double x2 = va[i + 1][0], y2 = va[i + 1][1], z2 = va[i + 1][2];
                for (int j = 0; j + 1 < vb.Count; j++)
                {
                    double x3 = vb[j][0], y3 = vb[j][1], z3 = vb[j][2];
                    double x4 = vb[j + 1][0], y4 = vb[j + 1][1], z4 = vb[j + 1][2];
                    double denom = (x1 - x2) * (y3 - y4) - (y1 - y2) * (x3 - x4);
                    if (Math.Abs(denom) < 1e-9) continue;
                    double t = ((x1 - x3) * (y3 - y4) - (y1 - y3) * (x3 - x4)) / denom;
                    double u = -((x1 - x2) * (y1 - y3) - (y1 - y2) * (x1 - x3)) / denom;
                    if (t >= -1e-6 && t <= 1.0 + 1e-6 && u >= -1e-6 && u <= 1.0 + 1e-6)
                    {
                        hits.Add(x1 + t * (x2 - x1));
                        hits.Add(y1 + t * (y2 - y1));
                        hits.Add(z1 + t * (z2 - z1));
                    }
                }
            }
            return hits.ToArray();
        }
    }

    public class AcadSpaceProxy : AcadProxyBase
    {
        private readonly bool _isPaper;

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public AcadSpaceProxy(bool isPaper)
        {
            _isPaper = isPaper;
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        protected override object HandleMember(string name, object[] args)
        {
            switch (name)
            {
                case "AddLightWeightPolyline":
                    if (args.Length >= 1)
                    {
                        double[] coords = AcadDynamicTarget.ToDblArray(args[0]);
                        var d = BridgeClient.Bridge("add_lwpolyline", new Dictionary<string, object>
                        {
                            { "coordinates", coords }
                        });
                        return AcadEntityProxy.FromJson(d);
                    }
                    return null;
                case "Add3DPoly":
                    if (args.Length >= 1)
                    {
                        double[] coords = AcadDynamicTarget.ToDblArray(args[0]);
                        var d = BridgeClient.Bridge("add_3dpoly", new Dictionary<string, object>
                        {
                            { "coordinates", coords }
                        });
                        return AcadEntityProxy.FromJson(d);
                    }
                    return null;
                case "AddLine":
                    if (args.Length >= 2)
                    {
                        double[] a = AcadDynamicTarget.ToDblArray(args[0]);
                        double[] b = AcadDynamicTarget.ToDblArray(args[1]);
                        var d = BridgeClient.Bridge("add_line", new Dictionary<string, object>
                        {
                            { "start", a },
                            { "end", b }
                        });
                        return AcadEntityProxy.FromJson(d);
                    }
                    return null;
                case "AddText":
                    if (args.Length >= 3)
                    {
                        string txt = Convert.ToString(args[0]);
                        double[] pt = AcadDynamicTarget.ToDblArray(args[1]);
                        double h = Convert.ToDouble(args[2], CultureInfo.InvariantCulture);
                        var d = BridgeClient.Bridge("add_text", new Dictionary<string, object>
                        {
                            { "text", txt },
                            { "point", pt },
                            { "height", h }
                        });
                        return AcadEntityProxy.FromJson(d);
                    }
                    return null;
                case "InsertBlock":
                    if (args.Length >= 6)
                    {
                        double[] pt = AcadDynamicTarget.ToDblArray(args[0]);
                        string bname = Convert.ToString(args[1]);
                        double xs = Convert.ToDouble(args[2], CultureInfo.InvariantCulture);
                        double ys = Convert.ToDouble(args[3], CultureInfo.InvariantCulture);
                        double zs = Convert.ToDouble(args[4], CultureInfo.InvariantCulture);
                        double rot = Convert.ToDouble(args[5], CultureInfo.InvariantCulture);
                        var d = BridgeClient.Bridge("insert_block", new Dictionary<string, object>
                        {
                            { "point", pt },
                            { "name", bname },
                            { "xscale", xs },
                            { "yscale", ys },
                            { "zscale", zs },
                            { "rotation", rot },
                            { "paperSpace", _isPaper }
                        });
                        return AcadEntityProxy.FromJson(d);
                    }
                    return null;
                default:
                    return null;
            }
        }
    }

    public class AcadLayerProxy : AcadProxyBase
    {
        public string LayerName;
        public int ColorIndex = 7;

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public AcadLayerProxy(string name)
        {
            LayerName = name;
            Target.Name = name;
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        protected override object HandleMember(string name, object[] args)
        {
            switch (name)
            {
                case "get_Name":
                    return LayerName;
                case "set_Name":
                    if (args.Length > 0) LayerName = Convert.ToString(args[0]);
                    return null;
                case "get_TrueColor":
                    return new AcadColorProxy(ColorIndex).GetTransparentProxy();
                case "set_TrueColor":
                    if (args.Length > 0 && args[0] != null)
                    {
                        var rp = RemotingServices.GetRealProxy(args[0]) as AcadColorProxy;
                        if (rp != null) ColorIndex = rp.ColorIndex;
                        BridgeClient.Bridge("ensure_layer", new Dictionary<string, object>
                        {
                            { "name", LayerName },
                            { "color", ColorIndex }
                        });
                    }
                    return null;
                case "get_color":
                case "get_Color":
                    return ColorIndex;
                case "set_color":
                case "set_Color":
                    if (args.Length > 0)
                    {
                        try
                        {
                            ColorIndex = Convert.ToInt32(args[0]);
                            BridgeClient.Bridge("ensure_layer", new Dictionary<string, object>
                            {
                                { "name", LayerName },
                                { "color", ColorIndex }
                            });
                        }
                        catch { }
                    }
                    return null;
                default:
                    return null;
            }
        }
    }

    public class AcadLayersProxy : AcadProxyBase
    {
        /// <summary>AutoCAD .NET / COM interop member.</summary>
        protected override object HandleMember(string name, object[] args)
        {
            if (name == "GetEnumerator")
            {
                return (new object[] { new AcadLayerProxy("0").GetTransparentProxy() }).GetEnumerator();
            }
            if ((name == "Add" || name == "Item" || name == "get_Item") && args.Length > 0)
            {
                string lname = Convert.ToString(args[0]);
                BridgeClient.Bridge("ensure_layer", new Dictionary<string, object> { { "name", lname } });
                return new AcadLayerProxy(lname).GetTransparentProxy();
            }
            return null;
        }
    }

    public class AcadSelectionSetProxy : AcadProxyBase
    {
        public string SetName;
        public List<object> Entities = new List<object>();

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public AcadSelectionSetProxy(string name)
        {
            SetName = name;
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        private void PopulateFromEntitiesJson(Dictionary<string, object> res)
        {
            Entities.Clear();
            if (res != null && res.ContainsKey("entities") && res["entities"] is IEnumerable)
            {
                foreach (var item in (IEnumerable)res["entities"])
                {
                    if (item is Dictionary<string, object>)
                    {
                        var ent = AcadEntityProxy.FromJson((Dictionary<string, object>)item);
                        if (ent != null) Entities.Add(ent);
                    }
                }
            }
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        protected override object HandleMember(string name, object[] args)
        {
            switch (name)
            {
                case "get_Name":
                    return SetName;
                case "get_Count":
                    return Entities.Count;
                case "Item":
                case "get_Item":
                    if (args.Length > 0)
                    {
                        int idx = Convert.ToInt32(args[0]);
                        if (idx >= 0 && idx < Entities.Count) return Entities[idx];
                    }
                    return null;
                case "GetEnumerator":
                    return Entities.GetEnumerator();
                case "Clear":
                case "Delete":
                    Entities.Clear();
                    return null;
                case "AddItems":
                    if (args.Length > 0 && args[0] is IEnumerable)
                    {
                        foreach (var item in (IEnumerable)args[0])
                        {
                            if (item != null) Entities.Add(item);
                        }
                    }
                    return null;
                case "Select":
                    {
                        string appFilter = "";
                        if (args.Length >= 5 && args[3] is IEnumerable && args[4] is IEnumerable)
                        {
                            var codes = new List<int>();
                            foreach (var c in (IEnumerable)args[3]) codes.Add(Convert.ToInt32(c));
                            var vals = new List<string>();
                            foreach (var v in (IEnumerable)args[4]) vals.Add(v != null ? Convert.ToString(v) : "");
                            for (int i = 0; i < Math.Min(codes.Count, vals.Count); i++)
                            {
                                if (codes[i] == 1001) appFilter = vals[i];
                            }
                        }
                        var res = BridgeClient.Bridge("select_all", new Dictionary<string, object> { { "app", appFilter } });
                        PopulateFromEntitiesJson(res);
                        return null;
                    }
                case "SelectByPolygon":
                    {
                        string appFilter = "";
                        if (args.Length >= 4 && args[2] is IEnumerable && args[3] is IEnumerable)
                        {
                            var codes = new List<int>();
                            foreach (var c in (IEnumerable)args[2]) codes.Add(Convert.ToInt32(c));
                            var vals = new List<string>();
                            foreach (var v in (IEnumerable)args[3]) vals.Add(v != null ? Convert.ToString(v) : "");
                            for (int i = 0; i < Math.Min(codes.Count, vals.Count); i++)
                            {
                                if (codes[i] == 1001) appFilter = vals[i];
                            }
                        }
                        var res = BridgeClient.Bridge("select_all", new Dictionary<string, object> { { "app", appFilter } });
                        PopulateFromEntitiesJson(res);
                        return null;
                    }
                case "SelectOnScreen":
                    {
                        var pick = BridgeClient.InteractivePick("selection", "Selecteer objecten in de tekening (Enter om te bevestigen)");
                        Entities.Clear();
                        if (pick != null && pick.ContainsKey("handles") && pick["handles"] is IEnumerable)
                        {
                            foreach (var hObj in (IEnumerable)pick["handles"])
                            {
                                string h = Convert.ToString(hObj);
                                if (!string.IsNullOrEmpty(h))
                                {
                                    var ed = BridgeClient.Bridge("entity", new Dictionary<string, object> { { "handle", h } });
                                    var ent = AcadEntityProxy.FromJson(ed);
                                    if (ent != null) Entities.Add(ent);
                                }
                            }
                        }
                        return null;
                    }
                default:
                    return null;
            }
        }
    }

    public class AcadSelectionSetsProxy : AcadProxyBase
    {
        private readonly Dictionary<string, object> _sets = new Dictionary<string, object>(StringComparer.OrdinalIgnoreCase);

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        protected override object HandleMember(string name, object[] args)
        {
            switch (name)
            {
                case "get_Count":
                    return _sets.Count;
                case "GetEnumerator":
                    return (new List<object>(_sets.Values)).GetEnumerator();
                case "Add":
                    if (args.Length > 0)
                    {
                        string sname = Convert.ToString(args[0]);
                        var proxy = new AcadSelectionSetProxy(sname).GetTransparentProxy();
                        _sets[sname] = proxy;
                        return proxy;
                    }
                    return null;
                case "Item":
                case "get_Item":
                    if (args.Length > 0)
                    {
                        if (args[0] is int)
                        {
                            int idx = (int)args[0];
                            int i = 0;
                            foreach (var kv in _sets)
                            {
                                if (i == idx) return kv.Value;
                                i++;
                            }
                        }
                        string sname = Convert.ToString(args[0]);
                        if (_sets.ContainsKey(sname)) return _sets[sname];
                        var proxy = new AcadSelectionSetProxy(sname).GetTransparentProxy();
                        _sets[sname] = proxy;
                        return proxy;
                    }
                    return null;
                default:
                    return null;
            }
        }
    }

    public class AcadUtilityProxy : AcadProxyBase
    {
        /// <summary>AutoCAD .NET / COM interop member.</summary>
        protected override object HandleMember(string name, object[] args)
        {
            switch (name)
            {
                case "Prompt":
                    if (args.Length > 0 && args[0] != null)
                    {
                        string msg = Convert.ToString(args[0]).Trim();
                        if (!string.IsNullOrEmpty(msg))
                        {
                            BridgeClient.Call("command", new Dictionary<string, object>
                            {
                                { "id", "lisp" },
                                { "params", new Dictionary<string, object> { { "code", "(princ " + QuoteLisp(msg) + ") (princ)" } } }
                            });
                        }
                    }
                    return null;
                case "GetPoint":
                    {
                        double[] basePt = null;
                        string prompt = "Wijs punt aan in tekening";
                        if (args.Length >= 1 && args[0] is IEnumerable && !(args[0] is string))
                        {
                            basePt = AcadDynamicTarget.ToDblArray(args[0]);
                        }
                        if (args.Length >= 2 && args[1] != null && !(args[1] is Missing))
                        {
                            prompt = Convert.ToString(args[1]);
                        }
                        else if (args.Length == 1 && args[0] is string)
                        {
                            prompt = (string)args[0];
                        }
                        var res = BridgeClient.InteractivePick("point", prompt, basePt);
                        if (BridgeClient.GetStr(res, "status") != "done")
                        {
                            throw new COMException("User cancelled GetPoint", -2147352567);
                        }
                        return BridgeClient.GetDblArray(res, "point", new double[] { 0.0, 0.0, 0.0 });
                    }
                case "GetEntity":
                    {
                        string prompt = args.Length >= 3 && args[2] != null ? Convert.ToString(args[2]) : "Selecteer een object";
                        var res = BridgeClient.InteractivePick("entity", prompt);
                        string h = BridgeClient.GetStr(res, "handle", "");
                        if (BridgeClient.GetStr(res, "status") != "done" || string.IsNullOrEmpty(h))
                        {
                            throw new COMException("No entity selected", -2147352567);
                        }
                        var ed = BridgeClient.Bridge("entity", new Dictionary<string, object> { { "handle", h } });
                        if (args.Length >= 2)
                        {
                            args[0] = AcadEntityProxy.FromJson(ed);
                            args[1] = BridgeClient.GetDblArray(res, "point", new double[] { 0.0, 0.0, 0.0 });
                        }
                        return null;
                    }
                case "GetDistance":
                    {
                        double[] basePt = null;
                        if (args.Length >= 1 && args[0] is IEnumerable && !(args[0] is string))
                        {
                            basePt = AcadDynamicTarget.ToDblArray(args[0]);
                        }
                        string prompt = args.Length >= 2 && args[1] != null ? Convert.ToString(args[1]) : "Geef afstand of tweede punt";
                        var res = BridgeClient.InteractivePick("distance", prompt, basePt);
                        if (BridgeClient.GetStr(res, "status") != "done")
                        {
                            throw new COMException("User cancelled GetDistance", -2147352567);
                        }
                        return BridgeClient.GetDbl(res, "value", 1.0);
                    }
                case "GetAngle":
                    {
                        double[] basePt = null;
                        if (args.Length >= 1 && args[0] is IEnumerable && !(args[0] is string))
                        {
                            basePt = AcadDynamicTarget.ToDblArray(args[0]);
                        }
                        string prompt = args.Length >= 2 && args[1] != null ? Convert.ToString(args[1]) : "Geef hoek of tweede punt";
                        var res = BridgeClient.InteractivePick("angle", prompt, basePt);
                        if (BridgeClient.GetStr(res, "status") != "done")
                        {
                            throw new COMException("User cancelled GetAngle", -2147352567);
                        }
                        return BridgeClient.GetDbl(res, "value", 0.0);
                    }
                case "PolarPoint":
                    if (args.Length >= 3)
                    {
                        double[] pt = AcadDynamicTarget.ToDblArray(args[0]);
                        double ang = Convert.ToDouble(args[1], CultureInfo.InvariantCulture);
                        double dist = Convert.ToDouble(args[2], CultureInfo.InvariantCulture);
                        double z = pt.Length > 2 ? pt[2] : 0.0;
                        return new double[]
                        {
                            pt[0] + Math.Cos(ang) * dist,
                            pt[1] + Math.Sin(ang) * dist,
                            z
                        };
                    }
                    return new double[] { 0.0, 0.0, 0.0 };
                case "AngleFromXAxis":
                    if (args.Length >= 2)
                    {
                        double[] p1 = AcadDynamicTarget.ToDblArray(args[0]);
                        double[] p2 = AcadDynamicTarget.ToDblArray(args[1]);
                        double ang = Math.Atan2(p2[1] - p1[1], p2[0] - p1[0]);
                        if (ang < 0.0) ang += Math.PI * 2.0;
                        return ang;
                    }
                    return 0.0;
                case "TranslateCoordinates":
                    if (args.Length >= 1)
                    {
                        return AcadDynamicTarget.ToDblArray(args[0]);
                    }
                    return new double[] { 0.0, 0.0, 0.0 };
                default:
                    return null;
            }
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        private static string QuoteLisp(string s)
        {
            return "\"" + (s ?? "").Replace("\\", "\\\\").Replace("\"", "\\\"") + "\"";
        }
    }

    public class AcadDocumentProxy : AcadProxyBase
    {
        private readonly object _modelSpace = new AcadSpaceProxy(false).GetTransparentProxy();
        private readonly object _paperSpace = new AcadSpaceProxy(true).GetTransparentProxy();
        private readonly object _layers = new AcadLayersProxy().GetTransparentProxy();
        private readonly object _selectionSets = new AcadSelectionSetsProxy().GetTransparentProxy();
        private readonly object _utility = new AcadUtilityProxy().GetTransparentProxy();

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        protected override object HandleMember(string name, object[] args)
        {
            switch (name)
            {
                case "get_ModelSpace":
                    return _modelSpace;
                case "get_PaperSpace":
                    return _paperSpace;
                case "get_Layers":
                    return _layers;
                case "get_SelectionSets":
                    return _selectionSets;
                case "get_Utility":
                    return _utility;
                case "get_ActiveLayer":
                    {
                        var st = BridgeClient.Bridge("state");
                        string cl = BridgeClient.GetStr(st, "clayer", "0");
                        return new AcadLayerProxy(cl).GetTransparentProxy();
                    }
                case "set_ActiveLayer":
                    return null;
                case "get_ActiveSpace":
                    {
                        var st = BridgeClient.Bridge("state");
                        bool mspace = BridgeClient.GetBool(st, "mspace", true);
                        return mspace ? 1 : 0; // acModelSpace = 1, acPaperSpace = 0
                    }
                case "set_ActiveSpace":
                    return null;
                case "get_MSpace":
                    return true;
                case "set_MSpace":
                    return null;
                case "GetVariable":
                    if (args.Length > 0)
                    {
                        string vname = Convert.ToString(args[0]).ToUpperInvariant();
                        var st = BridgeClient.Bridge("state");
                        if (vname == "DWGNAME") return BridgeClient.GetStr(st, "dwgname", "Drawing1.dwg");
                        if (vname == "DWGPREFIX") return BridgeClient.GetStr(st, "dwgprefix", Environment.CurrentDirectory);
                        if (vname == "CLAYER") return BridgeClient.GetStr(st, "clayer", "0");
                        if (vname == "OSMODE") return BridgeClient.GetInt(st, "osmode", 0);
                        return "";
                    }
                    return "";
                case "SetVariable":
                    return null;
                case "Regen":
                    return null;
                case "SendCommand":
                    if (args.Length > 0 && args[0] != null)
                    {
                        string cmdText = Convert.ToString(args[0]).Trim();
                        if (!string.IsNullOrEmpty(cmdText))
                        {
                            BridgeClient.Call("cmdline.submit", new Dictionary<string, object> { { "text", cmdText } });
                        }
                    }
                    return null;
                case "HandleToObject":
                    if (args.Length > 0 && args[0] != null)
                    {
                        string h = Convert.ToString(args[0]).Trim();
                        var ed = BridgeClient.Bridge("entity", new Dictionary<string, object> { { "handle", h } });
                        return AcadEntityProxy.FromJson(ed);
                    }
                    return null;
                default:
                    return null;
            }
        }
    }

    public sealed class PluginLocationAssembly : Assembly
    {
        private readonly Assembly _inner;
        private readonly string _location;

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public PluginLocationAssembly(Assembly inner, string location)
        {
            _inner = inner;
            _location = location ?? "";
        }

        public override string Location
        {
            get { return _location; }
        }

        public override string FullName
        {
            get { return _inner != null ? _inner.FullName : base.FullName; }
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public override AssemblyName GetName(bool copiedName)
        {
            return _inner != null ? _inner.GetName(copiedName) : new AssemblyName("Plugin");
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public override object[] GetCustomAttributes(Type attributeType, bool inherit)
        {
            return _inner != null ? _inner.GetCustomAttributes(attributeType, inherit) : new object[0];
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public override object[] GetCustomAttributes(bool inherit)
        {
            return _inner != null ? _inner.GetCustomAttributes(inherit) : new object[0];
        }
    }

    internal static class Program
    {
        private static readonly JavaScriptSerializer Json = new JavaScriptSerializer { MaxJsonLength = int.MaxValue };
        private static string _pluginDir = "";

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        [STAThread]
        private static int Main(string[] args)
        {
            System.Windows.Forms.Application.EnableVisualStyles();
            System.Windows.Forms.Application.SetCompatibleTextRenderingDefault(false);

            AppDomain.CurrentDomain.AssemblyResolve += OnAssemblyResolve;

            if (args.Length == 0)
            {
                Console.WriteLine("{\"ok\":false,\"error\":\"Usage: cadcraft-nethost <inspect|run|compile> ...\"}");
                return 1;
            }

            string mode = args[0].ToLowerInvariant();
            try
            {
                if (mode == "inspect" && args.Length >= 2)
                {
                    return DoInspect(args[1]);
                }
                if (mode == "run" && args.Length >= 3)
                {
                    int port = 0;
                    if (args.Length >= 4)
                    {
                        int.TryParse(args[3], NumberStyles.Integer, CultureInfo.InvariantCulture, out port);
                    }
                    return DoRun(args[1], args[2], port);
                }
                if (mode == "compile" && args.Length >= 3)
                {
                    return DoCompile(args[1], args[2]);
                }
                Console.WriteLine("{\"ok\":false,\"error\":\"Invalid arguments\"}");
                return 1;
            }
            catch (Exception ex)
            {
                var err = new Dictionary<string, object>
                {
                    { "ok", false },
                    { "error", ex.Message }
                };
                Console.WriteLine(Json.Serialize(err));
                return 1;
            }
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        private static Assembly OnAssemblyResolve(object sender, ResolveEventArgs args)
        {
            string shortName = new AssemblyName(args.Name).Name;
            if (shortName.Equals("Acmgd", StringComparison.OrdinalIgnoreCase) ||
                shortName.Equals("accoremgd", StringComparison.OrdinalIgnoreCase) ||
                shortName.Equals("Acdbmgd", StringComparison.OrdinalIgnoreCase) ||
                shortName.Equals("Autodesk.AutoCAD.Interop", StringComparison.OrdinalIgnoreCase) ||
                shortName.Equals("Autodesk.AutoCAD.Interop.Common", StringComparison.OrdinalIgnoreCase) ||
                shortName.Equals("System.Runtime", StringComparison.OrdinalIgnoreCase) ||
                shortName.Equals("System.Windows.Forms", StringComparison.OrdinalIgnoreCase) ||
                shortName.Equals("System.Windows.Forms.Primitives", StringComparison.OrdinalIgnoreCase) ||
                shortName.Equals("Microsoft.VisualBasic.Forms", StringComparison.OrdinalIgnoreCase) ||
                shortName.Equals("Microsoft.VisualBasic.Core", StringComparison.OrdinalIgnoreCase) ||
                shortName.Equals("System.Drawing.Common", StringComparison.OrdinalIgnoreCase) ||
                shortName.Equals("System.Drawing.Primitives", StringComparison.OrdinalIgnoreCase) ||
                shortName.Equals("System.ComponentModel.TypeConverter", StringComparison.OrdinalIgnoreCase) ||
                shortName.Equals("System.ComponentModel.Primitives", StringComparison.OrdinalIgnoreCase) ||
                shortName.Equals("System.Diagnostics.Process", StringComparison.OrdinalIgnoreCase) ||
                shortName.Equals("System.Resources.Extensions", StringComparison.OrdinalIgnoreCase) ||
                shortName.Equals("System.Collections", StringComparison.OrdinalIgnoreCase) ||
                shortName.Equals("System.Linq", StringComparison.OrdinalIgnoreCase) ||
                shortName.Equals("System.Threading", StringComparison.OrdinalIgnoreCase))
            {
                return typeof(Program).Assembly;
            }
            if (!string.IsNullOrEmpty(_pluginDir))
            {
                string cand = Path.Combine(_pluginDir, shortName + ".dll");
                if (File.Exists(cand))
                {
                    BridgeClient.UnblockFile(cand);
                    return LoadPluginAssembly(cand);
                }
            }
            return null;
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        private static void ReplaceBytesInPlace(byte[] buf, byte[] pat, byte[] rep)
        {
            if (buf == null || pat == null || rep == null || pat.Length == 0 || pat.Length != rep.Length) return;
            for (int i = 0; i <= buf.Length - pat.Length; i++)
            {
                bool match = true;
                for (int j = 0; j < pat.Length; j++)
                {
                    if (buf[i + j] != pat[j])
                    {
                        match = false;
                        break;
                    }
                }
                if (match)
                {
                    for (int j = 0; j < rep.Length; j++)
                    {
                        buf[i + j] = rep[j];
                    }
                }
            }
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        private static bool ContainsAscii(byte[] buf, string needle)
        {
            byte[] pat = Encoding.ASCII.GetBytes(needle);
            for (int i = 0; i <= buf.Length - pat.Length; i++)
            {
                bool match = true;
                for (int j = 0; j < pat.Length; j++)
                {
                    if (buf[i + j] != pat[j])
                    {
                        match = false;
                        break;
                    }
                }
                if (match) return true;
            }
            return false;
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        private static Assembly LoadPluginAssembly(string fullPath)
        {
            byte[] raw = File.ReadAllBytes(fullPath);
            if (!ContainsAscii(raw, ".NETCoreApp"))
            {
                return Assembly.LoadFrom(fullPath);
            }
            // Strip .NET 5/6/7/8 system assembly strong-name PublicKeyTokens in-memory so .NET Framework 4.8
            // routes AssemblyResolve to cadcraft-nethost's TypeForwardedTo table.
            byte[] zeroTok = new byte[9];
            ReplaceBytesInPlace(raw, new byte[] { 0x08, 0xB0, 0x3F, 0x5F, 0x7F, 0x11, 0xD5, 0x0A, 0x3A }, zeroTok);
            ReplaceBytesInPlace(raw, new byte[] { 0x08, 0xB7, 0x7A, 0x5C, 0x56, 0x19, 0x34, 0xE0, 0x89 }, zeroTok);
            ReplaceBytesInPlace(raw, new byte[] { 0x08, 0xCC, 0x7B, 0x13, 0xFF, 0xCD, 0x2D, 0xDD, 0x51 }, zeroTok);
            ReplaceBytesInPlace(
                raw,
                Encoding.ASCII.GetBytes("PublicKeyToken=cc7b13ffcd2ddd51"),
                Encoding.ASCII.GetBytes("PublicKeyToken=null            ")
            );
            // Rewrite .NET 8 2-arg String.Split(string/char, StringSplitOptions) calls to DisposableWrapper.Split
            ReplaceBytesInPlace(
                raw,
                new byte[] { 0x08, 0x20, 0x02, 0x1D, 0x0E, 0x0E, 0x11, 0x81, 0xCD },
                new byte[] { 0x07, 0x00, 0x03, 0x1D, 0x0E, 0x0E, 0x0E, 0x08, 0x00 }
            );
            ReplaceBytesInPlace(
                raw,
                new byte[] { 0x08, 0x20, 0x02, 0x1D, 0x0E, 0x03, 0x11, 0x81, 0xCD },
                new byte[] { 0x07, 0x00, 0x03, 0x1D, 0x0E, 0x0E, 0x03, 0x08, 0x00 }
            );
            ReplaceBytesInPlace(
                raw,
                new byte[] { 0x91, 0x03, 0xF0, 0x27, 0x7C, 0x01 },
                new byte[] { 0x69, 0x05, 0xF0, 0x27, 0x7C, 0x01 }
            );
            ReplaceBytesInPlace(
                raw,
                new byte[] { 0x91, 0x03, 0xF0, 0x27, 0x56, 0x03 },
                new byte[] { 0x69, 0x05, 0xF0, 0x27, 0x56, 0x03 }
            );
            ReplaceBytesInPlace(
                raw,
                new byte[] { 0x6F, 0x77, 0x00, 0x00, 0x0A },
                new byte[] { 0x28, 0x77, 0x00, 0x00, 0x0A }
            );
            ReplaceBytesInPlace(
                raw,
                new byte[] { 0x6F, 0xB5, 0x00, 0x00, 0x0A },
                new byte[] { 0x28, 0xB5, 0x00, 0x00, 0x0A }
            );
            return Assembly.Load(raw);
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        private static List<MethodInfo> DiscoverCommandMethods(Assembly asm, out Dictionary<string, MethodInfo> map)
        {
            map = new Dictionary<string, MethodInfo>(StringComparer.OrdinalIgnoreCase);
            var list = new List<MethodInfo>();
            Type[] types;
            try
            {
                types = asm.GetTypes();
            }
            catch (ReflectionTypeLoadException rtle)
            {
                types = rtle.Types;
            }
            if (types == null) return list;

            foreach (Type t in types)
            {
                if (t == null) continue;
                MethodInfo[] methods;
                try
                {
                    methods = t.GetMethods(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static | BindingFlags.Instance | BindingFlags.DeclaredOnly);
                }
                catch { continue; }

                foreach (MethodInfo m in methods)
                {
                    try
                    {
                        foreach (CustomAttributeData cad in CustomAttributeData.GetCustomAttributes(m))
                        {
                            string attrName = cad.Constructor.DeclaringType != null ? cad.Constructor.DeclaringType.Name : "";
                            if (attrName == "CommandMethodAttribute" || attrName == "CommandMethod")
                            {
                                foreach (var arg in cad.ConstructorArguments)
                                {
                                    if (arg.ArgumentType == typeof(string) && arg.Value != null)
                                    {
                                        string cmdName = Convert.ToString(arg.Value).Trim();
                                        if (!string.IsNullOrEmpty(cmdName) && !map.ContainsKey(cmdName))
                                        {
                                            map[cmdName] = m;
                                            list.Add(m);
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    catch { }
                }
            }
            return list;
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        private static int DoInspect(string dllPath)
        {
            string fullPath = Path.GetFullPath(dllPath);
            if (!File.Exists(fullPath))
            {
                Console.WriteLine(Json.Serialize(new Dictionary<string, object>
                {
                    { "ok", false },
                    { "error", "File not found: " + fullPath }
                }));
                return 1;
            }
            BridgeClient.UnblockFile(fullPath);
            _pluginDir = Path.GetDirectoryName(fullPath) ?? "";
            Assembly asm = LoadPluginAssembly(fullPath);
            Dictionary<string, MethodInfo> map;
            DiscoverCommandMethods(asm, out map);
            var cmds = new List<string>(map.Keys);
            cmds.Sort(StringComparer.OrdinalIgnoreCase);
            Console.WriteLine(Json.Serialize(new Dictionary<string, object>
            {
                { "ok", true },
                { "path", fullPath },
                { "commands", cmds }
            }));
            return 0;
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        private static void ConfigureVbApplicationInfo(Assembly asm, string fullPath)
        {
            try
            {
                Type[] types;
                try { types = asm.GetTypes(); }
                catch (ReflectionTypeLoadException rtle) { types = rtle.Types; }
                if (types == null) return;

                Type vbAppBase = Type.GetType("Microsoft.VisualBasic.ApplicationServices.ApplicationBase, Microsoft.VisualBasic, Version=10.0.0.0, Culture=neutral, PublicKeyToken=b03f5f7f11d50a3a");
                Type vbAsmInfo = Type.GetType("Microsoft.VisualBasic.ApplicationServices.AssemblyInfo, Microsoft.VisualBasic, Version=10.0.0.0, Culture=neutral, PublicKeyToken=b03f5f7f11d50a3a");
                if (vbAppBase == null || vbAsmInfo == null) return;

                FieldInfo mInfoField = vbAppBase.GetField("m_Info", BindingFlags.NonPublic | BindingFlags.Instance);
                if (mInfoField == null) return;

                Assembly locAsm = string.IsNullOrEmpty(asm.Location) ? new PluginLocationAssembly(asm, fullPath) : asm;
                object asmInfoInstance = Activator.CreateInstance(vbAsmInfo, new object[] { locAsm });
                foreach (Type t in types)
                {
                    if (t == null || t.Name != "MyProject") continue;
                    PropertyInfo appProp = t.GetProperty("Application", BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static);
                    if (appProp != null)
                    {
                        object appObj = appProp.GetValue(null, null);
                        if (appObj != null)
                        {
                            mInfoField.SetValue(appObj, asmInfoInstance);
                        }
                    }
                }
            }
            catch { }
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        private static int DoRun(string dllPath, string commandName, int port)
        {
            string fullPath = Path.GetFullPath(dllPath);
            if (!File.Exists(fullPath)) return 1;

            BridgeClient.UnblockFile(fullPath);
            _pluginDir = Path.GetDirectoryName(fullPath) ?? "";
            BridgeClient.CurrentDllDir = _pluginDir;
            BridgeClient.ControlPort = port;
            if (!string.IsNullOrEmpty(_pluginDir) && Directory.Exists(_pluginDir))
            {
                Environment.CurrentDirectory = _pluginDir;
            }

            AcadColorProxy.EnsureComRegistered();

            Assembly asm = LoadPluginAssembly(fullPath);
            ConfigureVbApplicationInfo(asm, fullPath);
            Dictionary<string, MethodInfo> map;
            DiscoverCommandMethods(asm, out map);

            MethodInfo target;
            if (!map.TryGetValue(commandName.Trim(), out target))
            {
                MessageBox.Show("Command '" + commandName + "' not found in " + Path.GetFileName(fullPath), "CADCraft NETLOAD", MessageBoxButtons.OK, MessageBoxIcon.Warning);
                return 1;
            }

            object instance = null;
            if (!target.IsStatic)
            {
                instance = Activator.CreateInstance(target.DeclaringType);
            }

            target.Invoke(instance, null);

            // Keep message loop alive while any WinForms windows or PaletteSets are open
            if (System.Windows.Forms.Application.OpenForms.Count > 0)
            {
                while (System.Windows.Forms.Application.OpenForms.Count > 0)
                {
                    bool anyVisible = false;
                    foreach (Form f in System.Windows.Forms.Application.OpenForms)
                    {
                        if (f.Visible)
                        {
                            anyVisible = true;
                            break;
                        }
                    }
                    if (!anyVisible) break;
                    System.Windows.Forms.Application.DoEvents();
                    Thread.Sleep(25);
                }
            }
            return 0;
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        private static int DoCompile(string srcPath, string outDllPath)
        {
            string fullSrc = Path.GetFullPath(srcPath);
            string fullOut = Path.GetFullPath(outDllPath);
            bool isVb = fullSrc.EndsWith(".vb", StringComparison.OrdinalIgnoreCase);
            string fwDir = RuntimeEnvironment.GetRuntimeDirectory();
            string compiler = Path.Combine(fwDir, isVb ? "vbc.exe" : "csc.exe");
            if (!File.Exists(compiler))
            {
                Console.WriteLine(Json.Serialize(new Dictionary<string, object>
                {
                    { "ok", false },
                    { "error", "Compiler not found: " + compiler }
                }));
                return 1;
            }

            string selfExe = typeof(Program).Assembly.Location;
            var psi = new ProcessStartInfo
            {
                FileName = compiler,
                Arguments = string.Format("/nologo /target:library /out:\"{0}\" /r:\"{1}\" /r:System.dll /r:System.Drawing.dll /r:System.Windows.Forms.dll /r:Microsoft.CSharp.dll \"{2}\"", fullOut, selfExe, fullSrc),
                UseShellExecute = false,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                CreateNoWindow = true
            };
            using (var proc = Process.Start(psi))
            {
                string stdout = proc.StandardOutput.ReadToEnd();
                string stderr = proc.StandardError.ReadToEnd();
                proc.WaitForExit();
                if (proc.ExitCode == 0 && File.Exists(fullOut))
                {
                    Console.WriteLine(Json.Serialize(new Dictionary<string, object>
                    {
                        { "ok", true },
                        { "dll", fullOut }
                    }));
                    return 0;
                }
                else
                {
                    Console.WriteLine(Json.Serialize(new Dictionary<string, object>
                    {
                        { "ok", false },
                        { "error", (stdout + "\n" + stderr).Trim() }
                    }));
                    return 1;
                }
            }
        }
    }
}

namespace System.Resources.Extensions
{
    public class DeserializingResourceReader : IResourceReader
    {
        private readonly Hashtable _table = new Hashtable();

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public DeserializingResourceReader(Stream stream)
        {
            LoadFromStream(stream);
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public DeserializingResourceReader(string fileName)
        {
            if (!string.IsNullOrEmpty(fileName) && File.Exists(fileName))
            {
                using (var fs = File.OpenRead(fileName))
                {
                    LoadFromStream(fs);
                }
            }
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        private void LoadFromStream(Stream stream)
        {
            try
            {
                if (stream == null) return;
                using (var ms = new MemoryStream())
                {
                    stream.CopyTo(ms);
                    byte[] buf = ms.ToArray();
                    // Scan for embedded .ico (00 00 01 00 01 00) if $this.Icon is present
                    for (int i = 180; i + 22 < buf.Length; i++)
                    {
                        if (buf[i] == 0x00 && buf[i + 1] == 0x00 && buf[i + 2] == 0x01 && buf[i + 3] == 0x00 &&
                            buf[i + 4] == 0x01 && buf[i + 5] == 0x00)
                        {
                            try
                            {
                                using (var icoMs = new MemoryStream(buf, i, buf.Length - i))
                                {
                                    _table["$this.Icon"] = new Icon(icoMs);
                                    break;
                                }
                            }
                            catch { }
                        }
                    }
                }
            }
            catch { }
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public void Close() { }
        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public void Dispose() { }
        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public IDictionaryEnumerator GetEnumerator() { return _table.GetEnumerator(); }
        IEnumerator IEnumerable.GetEnumerator() { return _table.GetEnumerator(); }
    }

    public class RuntimeResourceSet : ResourceSet
    {
        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public RuntimeResourceSet(IResourceReader reader) : base(reader) { }
        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public RuntimeResourceSet(Stream stream) : base(new DeserializingResourceReader(stream)) { }
        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public RuntimeResourceSet(string fileName) : base(new DeserializingResourceReader(fileName)) { }
    }
}

namespace Autodesk.AutoCAD.Interop.Common
{
    using CadCraft.NetHost;

    public enum AcActiveSpace
    {
        acPaperSpace = 0,
        acModelSpace = 1
    }

    public enum AcAlignment
    {
        acAlignmentLeft = 0,
        acAlignmentCenter = 1,
        acAlignmentRight = 2,
        acAlignmentAligned = 3,
        acAlignmentMiddle = 4,
        acAlignmentFit = 5,
        acAlignmentTopLeft = 6,
        acAlignmentTopCenter = 7,
        acAlignmentTopRight = 8,
        acAlignmentMiddleLeft = 9,
        acAlignmentMiddleCenter = 10,
        acAlignmentMiddleRight = 11,
        acAlignmentBottomLeft = 12,
        acAlignmentBottomCenter = 13,
        acAlignmentBottomRight = 14
    }

    public enum AcColor
    {
        acByBlock = 0,
        acRed = 1,
        acYellow = 2,
        acGreen = 3,
        acCyan = 4,
        acBlue = 5,
        acMagenta = 6,
        acWhite = 7,
        acByLayer = 256
    }

    public enum ACAD_COLOR
    {
        acByBlock = 0,
        acRed = 1,
        acYellow = 2,
        acGreen = 3,
        acCyan = 4,
        acBlue = 5,
        acMagenta = 6,
        acWhite = 7,
        acByLayer = 256
    }

    public enum AcColorMethod
    {
        acColorMethodByLayer = 192,
        acColorMethodByBlock = 193,
        acColorMethodByRGB = 194,
        acColorMethodByACI = 195,
        acColorMethodForeground = 197
    }

    public enum AcCoordinateSystem
    {
        acWorld = 0,
        acUCS = 1,
        acDisplayDCS = 2,
        acPaperSpaceDCS = 3,
        acOCS = 4
    }

    public enum AcExtendOption
    {
        acExtendNone = 0,
        acExtendThisEntity = 1,
        acExtendOtherEntity = 2,
        acExtendBoth = 3
    }

    public enum AcLineWeight
    {
        acLnWt000 = 0,
        acLnWt005 = 5,
        acLnWt009 = 9,
        acLnWt013 = 13,
        acLnWt015 = 15,
        acLnWt018 = 18,
        acLnWt020 = 20,
        acLnWt025 = 25,
        acLnWt030 = 30,
        acLnWt035 = 35,
        acLnWt040 = 40,
        acLnWt050 = 50,
        acLnWt053 = 53,
        acLnWt060 = 60,
        acLnWt070 = 70,
        acLnWt080 = 80,
        acLnWt090 = 90,
        acLnWt100 = 100,
        acLnWt106 = 106,
        acLnWt120 = 120,
        acLnWt140 = 140,
        acLnWt158 = 158,
        acLnWt200 = 200,
        acLnWt211 = 211,
        acLnWtByLayer = -1,
        acLnWtByBlock = -2,
        acLnWtByLwDefault = -3
    }

    public enum ACAD_LWEIGHT
    {
        acLnWt000 = 0,
        acLnWt005 = 5,
        acLnWt009 = 9,
        acLnWt013 = 13,
        acLnWt015 = 15,
        acLnWt018 = 18,
        acLnWt020 = 20,
        acLnWt025 = 25,
        acLnWt030 = 30,
        acLnWt035 = 35,
        acLnWt040 = 40,
        acLnWt050 = 50,
        acLnWt053 = 53,
        acLnWt060 = 60,
        acLnWt070 = 70,
        acLnWt080 = 80,
        acLnWt090 = 90,
        acLnWt100 = 100,
        acLnWt106 = 106,
        acLnWt120 = 120,
        acLnWt140 = 140,
        acLnWt158 = 158,
        acLnWt200 = 200,
        acLnWt211 = 211,
        acLnWtByLayer = -1,
        acLnWtByBlock = -2,
        acLnWtByLwDefault = -3
    }

    public enum AcRegenType
    {
        acActiveViewport = 0,
        acAllViewports = 1
    }

    public enum AcSelect
    {
        acSelectionSetWindow = 0,
        acSelectionSetCrossing = 1,
        acSelectionSetFence = 2,
        acSelectionSetPrevious = 3,
        acSelectionSetLast = 4,
        acSelectionSetAll = 5,
        acSelectionSetWindowPolygon = 6,
        acSelectionSetCrossingPolygon = 7
    }

    public interface IAcadAcCmColor
    {
        int ColorIndex { get; set; }
        AcColorMethod ColorMethod { get; set; }
    }

    public interface AcadAcCmColor : IAcadAcCmColor { }

    [AcadColorProxyAttr]
    public class AcadAcCmColorClass : ContextBoundObject, AcadAcCmColor
    {
        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public AcadAcCmColorClass() { }
        public virtual int ColorIndex { get; set; }
        public virtual AcColorMethod ColorMethod { get; set; }
    }

    public interface IAcadObject
    {
        string Handle { get; }
        string ObjectName { get; }
        void Delete();
        void GetXData(string AppName, out object XDataType, out object XDataValue);
        void SetXData(object XDataType, object XDataValue);
    }

    public interface AcadObject : IAcadObject { }

    public interface IAcadEntity : AcadObject
    {
        new string Handle { get; }
        new string ObjectName { get; }
        string EntityName { get; }
        new void Delete();
        new void GetXData(string AppName, out object XDataType, out object XDataValue);
        new void SetXData(object XDataType, object XDataValue);
        string Layer { get; set; }
        ACAD_LWEIGHT Lineweight { get; set; }
        AcadAcCmColor TrueColor { get; set; }
        ACAD_COLOR color { get; set; }
        void GetBoundingBox(out object MinPoint, out object MaxPoint);
        void Highlight(bool HighlightFlag);
        object IntersectWith(object IntersectObject, AcExtendOption option);
        void Rotate(object BasePoint, double RotationAngle);
        void Update();
    }

    public interface AcadEntity : IAcadEntity { }

    public interface IAcadLWPolyline : AcadEntity
    {
        new string Handle { get; }
        new string ObjectName { get; }
        new string EntityName { get; }
        new void Delete();
        new void GetXData(string AppName, out object XDataType, out object XDataValue);
        new void SetXData(object XDataType, object XDataValue);
        new string Layer { get; set; }
        new ACAD_LWEIGHT Lineweight { get; set; }
        new AcadAcCmColor TrueColor { get; set; }
        new ACAD_COLOR color { get; set; }
        new void GetBoundingBox(out object MinPoint, out object MaxPoint);
        new void Highlight(bool HighlightFlag);
        new object IntersectWith(object IntersectObject, AcExtendOption option);
        new void Rotate(object BasePoint, double RotationAngle);
        new void Update();
        bool Closed { get; set; }
        double ConstantWidth { get; set; }
        object Coordinates { get; set; }
        object get_Coordinate(int Index);
        void set_Coordinate(int Index, object pVal);
        object Offset(double Distance);
    }

    public interface AcadLWPolyline : IAcadLWPolyline { }

    public interface IAcad3DPolyline : AcadEntity
    {
        new string Handle { get; }
        new string ObjectName { get; }
        new string EntityName { get; }
        new void Delete();
        new void GetXData(string AppName, out object XDataType, out object XDataValue);
        new void SetXData(object XDataType, object XDataValue);
        new string Layer { get; set; }
        new ACAD_LWEIGHT Lineweight { get; set; }
        new AcadAcCmColor TrueColor { get; set; }
        new ACAD_COLOR color { get; set; }
        new void GetBoundingBox(out object MinPoint, out object MaxPoint);
        new void Highlight(bool HighlightFlag);
        new object IntersectWith(object IntersectObject, AcExtendOption option);
        new void Rotate(object BasePoint, double RotationAngle);
        new void Update();
        bool Closed { get; set; }
        object Coordinates { get; set; }
        object get_Coordinate(int Index);
        void set_Coordinate(int Index, object pVal);
    }

    public interface Acad3DPolyline : IAcad3DPolyline { }

    public interface IAcadLine : AcadEntity
    {
        new string Handle { get; }
        new string ObjectName { get; }
        new string EntityName { get; }
        new void Delete();
        new void GetXData(string AppName, out object XDataType, out object XDataValue);
        new void SetXData(object XDataType, object XDataValue);
        new string Layer { get; set; }
        new ACAD_LWEIGHT Lineweight { get; set; }
        new AcadAcCmColor TrueColor { get; set; }
        new ACAD_COLOR color { get; set; }
        new void GetBoundingBox(out object MinPoint, out object MaxPoint);
        new void Highlight(bool HighlightFlag);
        new object IntersectWith(object IntersectObject, AcExtendOption option);
        new void Rotate(object BasePoint, double RotationAngle);
        new void Update();
        object StartPoint { get; set; }
        object EndPoint { get; set; }
        object Offset(double Distance);
    }

    public interface AcadLine : IAcadLine { }

    public interface IAcadText : AcadEntity
    {
        new string Handle { get; }
        new string ObjectName { get; }
        new string EntityName { get; }
        new void Delete();
        new void GetXData(string AppName, out object XDataType, out object XDataValue);
        new void SetXData(object XDataType, object XDataValue);
        new string Layer { get; set; }
        new ACAD_LWEIGHT Lineweight { get; set; }
        new AcadAcCmColor TrueColor { get; set; }
        new ACAD_COLOR color { get; set; }
        new void GetBoundingBox(out object MinPoint, out object MaxPoint);
        new void Highlight(bool HighlightFlag);
        new object IntersectWith(object IntersectObject, AcExtendOption option);
        new void Rotate(object BasePoint, double RotationAngle);
        new void Update();
        AcAlignment Alignment { get; set; }
        object InsertionPoint { get; set; }
        double Rotation { get; set; }
        object TextAlignmentPoint { get; set; }
        string TextString { get; set; }
    }

    public interface AcadText : IAcadText { }

    public interface IAcadBlockReference : AcadEntity
    {
        new string Handle { get; }
        new string ObjectName { get; }
        new string EntityName { get; }
        new void Delete();
        new void GetXData(string AppName, out object XDataType, out object XDataValue);
        new void SetXData(object XDataType, object XDataValue);
        new string Layer { get; set; }
        new ACAD_LWEIGHT Lineweight { get; set; }
        new AcadAcCmColor TrueColor { get; set; }
        new ACAD_COLOR color { get; set; }
        new void GetBoundingBox(out object MinPoint, out object MaxPoint);
        new void Highlight(bool HighlightFlag);
        new object IntersectWith(object IntersectObject, AcExtendOption option);
        new void Rotate(object BasePoint, double RotationAngle);
        new void Update();
        string EffectiveName { get; }
        object InsertionPoint { get; set; }
        string Name { get; set; }
        double Rotation { get; set; }
    }

    public interface AcadBlockReference : IAcadBlockReference { }

    public interface IAcadLayer : AcadObject
    {
        string Name { get; set; }
        AcadAcCmColor TrueColor { get; set; }
        AcColor color { get; set; }
    }

    public interface AcadLayer : IAcadLayer { }

    public interface IAcadLayers : IEnumerable
    {
        AcadLayer Add(string Name);
        AcadLayer Item(object Index);
        new IEnumerator GetEnumerator();
    }

    public interface AcadLayers : IAcadLayers { }

    public interface IAcadBlock
    {
        Acad3DPolyline Add3DPoly(object PointsArray);
        AcadLine AddLine(object StartPoint, object EndPoint);
        AcadLWPolyline AddLightWeightPolyline(object VerticesList);
        AcadText AddText(string TextString, object InsertionPoint, double Height);
        AcadBlockReference InsertBlock(object InsertionPoint, string Name, double Xscale, double Yscale, double Zscale, double Rotation, [Optional] object Password);
    }

    public interface IAcadModelSpace : IAcadBlock
    {
        new Acad3DPolyline Add3DPoly(object PointsArray);
        new AcadLine AddLine(object StartPoint, object EndPoint);
        new AcadLWPolyline AddLightWeightPolyline(object VerticesList);
        new AcadText AddText(string TextString, object InsertionPoint, double Height);
        new AcadBlockReference InsertBlock(object InsertionPoint, string Name, double Xscale, double Yscale, double Zscale, double Rotation, [Optional] object Password);
    }
    public interface AcadModelSpace : IAcadModelSpace { }

    public interface IAcadPaperSpace : IAcadBlock
    {
        new Acad3DPolyline Add3DPoly(object PointsArray);
        new AcadLine AddLine(object StartPoint, object EndPoint);
        new AcadLWPolyline AddLightWeightPolyline(object VerticesList);
        new AcadText AddText(string TextString, object InsertionPoint, double Height);
        new AcadBlockReference InsertBlock(object InsertionPoint, string Name, double Xscale, double Yscale, double Zscale, double Rotation, [Optional] object Password);
    }
    public interface AcadPaperSpace : IAcadPaperSpace { }
}

namespace Autodesk.AutoCAD.Interop
{
    using Autodesk.AutoCAD.Interop.Common;

    public interface IAcadSelectionSet : IEnumerable
    {
        int Count { get; }
        string Name { get; }
        AcadEntity Item(object Index);
        new IEnumerator GetEnumerator();
        void Clear();
        void Delete();
        void AddItems(object pObjs);
        void Select(AcSelect Mode, [Optional] object Point1, [Optional] object Point2, [Optional] object FilterType, [Optional] object FilterData);
        void SelectByPolygon(AcSelect Mode, object PointsList, [Optional] object FilterType, [Optional] object FilterData);
        void SelectOnScreen([Optional] object FilterType, [Optional] object FilterData);
    }

    public interface AcadSelectionSet : IAcadSelectionSet { }

    public interface IAcadSelectionSets : IEnumerable
    {
        int Count { get; }
        AcadSelectionSet Add(string Name);
        AcadSelectionSet Item(object Index);
        new IEnumerator GetEnumerator();
    }

    public interface AcadSelectionSets : IAcadSelectionSets { }

    public interface IAcadUtility
    {
        double AngleFromXAxis(object Point1, object Point2);
        double GetAngle([Optional] object Point, [Optional] object Prompt);
        double GetDistance([Optional] object Point, [Optional] object Prompt);
        void GetEntity(out object Object, out object PickedPoint, [Optional] object Prompt);
        object GetPoint([Optional] object Point, [Optional] object Prompt);
        object PolarPoint(object Point, double Angle, double Distance);
        void Prompt(string Message);
        object TranslateCoordinates(object Point, AcCoordinateSystem FromCoordSystem, AcCoordinateSystem ToCoordSystem, int Disp, [Optional] object OCSNormal);
    }

    public interface AcadUtility : IAcadUtility { }

    public interface IAcadDocument
    {
        AcadLayer ActiveLayer { get; set; }
        AcActiveSpace ActiveSpace { get; set; }
        AcadLayers Layers { get; }
        AcadModelSpace ModelSpace { get; }
        bool MSpace { get; set; }
        AcadPaperSpace PaperSpace { get; }
        AcadSelectionSets SelectionSets { get; }
        AcadUtility Utility { get; }
        object GetVariable(string Name);
        void SetVariable(string Name, object Value);
        object HandleToObject(string Handle);
        void Regen(AcRegenType WhichViewports);
        void SendCommand(string Command);
        void StartUndoMark();
        void EndUndoMark();
    }

    public interface AcadDocument : IAcadDocument { }
}

namespace Autodesk.AutoCAD.Runtime
{
    public class DisposableWrapper : IDisposable
    {
        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public virtual void Dispose() { }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public static string[] Split(string s, string sep, int options)
        {
            if (s == null) return new string[0];
            return s.Split(new string[] { sep }, (StringSplitOptions)options);
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public static string[] Split(string s, char sep, int options)
        {
            if (s == null) return new string[0];
            return s.Split(new char[] { sep }, (StringSplitOptions)options);
        }
    }

    public enum CommandFlags
    {
        Modal = 0,
        Transparent = 1,
        UsePickSet = 2,
        Redraw = 4,
        NoPerspective = 8,
        NoMultiple = 16,
        NoTileMode = 32,
        NoPaperSpace = 64,
        NoOem = 256,
        Defun = 512,
        InProgress = 1024,
        NoUndoMarker = 2048,
        NoBlockEditor = 4096,
        NoActionRecording = 8192,
        ActionMacro = 16384,
        NoHistory = 1048576,
        Session = 2097152,
        Interruptible = 4194304
    }

    [AttributeUsage(AttributeTargets.Method, AllowMultiple = true, Inherited = false)]
    public sealed class CommandMethodAttribute : Attribute
    {
        public string GlobalName { get; private set; }
        public string GroupName { get; private set; }
        public CommandFlags Flags { get; private set; }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public CommandMethodAttribute(string globalName)
        {
            GlobalName = globalName;
        }
        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public CommandMethodAttribute(string globalName, CommandFlags flags)
        {
            GlobalName = globalName;
            Flags = flags;
        }
        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public CommandMethodAttribute(string groupName, string globalName, CommandFlags flags)
        {
            GroupName = groupName;
            GlobalName = globalName;
            Flags = flags;
        }
        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public CommandMethodAttribute(string groupName, string globalName, string localizedNameId, CommandFlags flags)
        {
            GroupName = groupName;
            GlobalName = globalName;
            Flags = flags;
        }
        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public CommandMethodAttribute(string groupName, string globalName, string localizedNameId, CommandFlags flags, Type contextMenuExtensionType)
        {
            GroupName = groupName;
            GlobalName = globalName;
            Flags = flags;
        }
        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public CommandMethodAttribute(string groupName, string globalName, string localizedNameId, CommandFlags flags, string helpFileName, string helpTopic)
        {
            GroupName = groupName;
            GlobalName = globalName;
            Flags = flags;
        }
        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public CommandMethodAttribute(string groupName, string globalName, string localizedNameId, CommandFlags flags, Type contextMenuExtensionType, string helpFileName, string helpTopic)
        {
            GroupName = groupName;
            GlobalName = globalName;
            Flags = flags;
        }
    }
}

namespace Autodesk.AutoCAD.DatabaseServices
{
    using Autodesk.AutoCAD.Runtime;
    using CadCraft.NetHost;

    public enum OpenMode
    {
        ForRead = 0,
        ForWrite = 1,
        ForNotify = 2
    }

    public enum DxfCode
    {
        Invalid = -9999,
        Start = 0,
        Text = 1,
        LayerName = 8,
        ExtendedDataAsciiString = 1000,
        ExtendedDataRegAppName = 1001,
        ExtendedDataControlString = 1002,
        ExtendedDataLayerName = 1003,
        ExtendedDataBinaryChunk = 1004,
        ExtendedDataHandle = 1005,
        ExtendedDataXCoordinate = 1010,
        ExtendedDataWorldXCoordinate = 1011,
        ExtendedDataWorldXDisp = 1012,
        ExtendedDataWorldXDir = 1013,
        ExtendedDataReal = 1040,
        ExtendedDataDist = 1041,
        ExtendedDataScale = 1042,
        ExtendedDataInteger16 = 1070,
        ExtendedDataInteger32 = 1071
    }

    public struct Handle
    {
        private readonly long _val;

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public Handle(long value)
        {
            _val = value;
        }

        public long Value
        {
            get { return _val; }
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public override string ToString()
        {
            return _val.ToString("X", CultureInfo.InvariantCulture);
        }
    }

    public struct ObjectId
    {
        public string HandleHex;

        public static ObjectId Null
        {
            get { return default(ObjectId); }
        }

        public bool IsNull
        {
            get { return string.IsNullOrEmpty(HandleHex); }
        }

        public bool IsErased
        {
            get { return string.IsNullOrEmpty(HandleHex); }
        }

        public Handle Handle
        {
            get
            {
                long v = 0;
                if (!string.IsNullOrEmpty(HandleHex))
                {
                    if (!long.TryParse(HandleHex, NumberStyles.HexNumber, CultureInfo.InvariantCulture, out v))
                    {
                        long.TryParse(HandleHex, NumberStyles.Integer, CultureInfo.InvariantCulture, out v);
                    }
                }
                return new Handle(v);
            }
        }

        public static bool operator ==(ObjectId a, ObjectId b)
        {
            return string.Equals(a.HandleHex ?? "", b.HandleHex ?? "", StringComparison.OrdinalIgnoreCase);
        }

        public static bool operator !=(ObjectId a, ObjectId b)
        {
            return !(a == b);
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public override bool Equals(object obj)
        {
            return obj is ObjectId && this == (ObjectId)obj;
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public override int GetHashCode()
        {
            return (HandleHex ?? "").ToUpperInvariant().GetHashCode();
        }
    }

    public sealed class ObjectIdCollection : ICollection, IEnumerable
    {
        private readonly List<ObjectId> _list = new List<ObjectId>();

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public ObjectIdCollection() { }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public ObjectIdCollection(ObjectId[] ids)
        {
            if (ids != null) _list.AddRange(ids);
        }

        public int Count
        {
            get { return _list.Count; }
        }

        public ObjectId this[int index]
        {
            get { return _list[index]; }
            set { _list[index] = value; }
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public int Add(ObjectId value)
        {
            _list.Add(value);
            return _list.Count - 1;
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public bool Contains(ObjectId value)
        {
            return _list.Contains(value);
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public void Clear()
        {
            _list.Clear();
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public void CopyTo(ObjectId[] array, int index)
        {
            _list.CopyTo(array, index);
        }

        void ICollection.CopyTo(Array array, int index)
        {
            ((ICollection)_list).CopyTo(array, index);
        }

        bool ICollection.IsSynchronized
        {
            get { return false; }
        }

        object ICollection.SyncRoot
        {
            get { return this; }
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public IEnumerator GetEnumerator()
        {
            return _list.GetEnumerator();
        }
    }

    public struct TypedValue
    {
        private readonly short _typeCode;
        private readonly object _value;

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public TypedValue(int typeCode, object value)
        {
            _typeCode = (short)typeCode;
            _value = value;
        }

        public short TypeCode
        {
            get { return _typeCode; }
        }

        public object Value
        {
            get { return _value; }
        }
    }

    public sealed class ResultBufferEnumerator : IEnumerator
    {
        private readonly List<TypedValue> _items;
        private int _index = -1;

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public ResultBufferEnumerator(List<TypedValue> items)
        {
            _items = items ?? new List<TypedValue>();
        }

        public TypedValue Current
        {
            get { return _items[_index]; }
        }

        object IEnumerator.Current
        {
            get { return _items[_index]; }
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public bool MoveNext()
        {
            _index++;
            return _index < _items.Count;
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public void Reset()
        {
            _index = -1;
        }
    }

    public class ResultBuffer : DisposableWrapper, IEnumerable
    {
        internal readonly List<TypedValue> Items = new List<TypedValue>();

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public ResultBuffer() { }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public ResultBuffer(params TypedValue[] values)
        {
            if (values != null) Items.AddRange(values);
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public void Add(TypedValue value)
        {
            Items.Add(value);
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public TypedValue[] AsArray()
        {
            return Items.ToArray();
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public ResultBufferEnumerator GetEnumerator()
        {
            return new ResultBufferEnumerator(Items);
        }

        IEnumerator IEnumerable.GetEnumerator()
        {
            return new ResultBufferEnumerator(Items);
        }
    }

    public class DBObject : DisposableWrapper
    {
        public ObjectId ObjectId;

        public ObjectId Id
        {
            get { return ObjectId; }
        }

        public Handle Handle
        {
            get
            {
                long v = 0;
                if (!string.IsNullOrEmpty(ObjectId.HandleHex))
                {
                    if (!long.TryParse(ObjectId.HandleHex, NumberStyles.HexNumber, CultureInfo.InvariantCulture, out v))
                    {
                        long.TryParse(ObjectId.HandleHex, NumberStyles.Integer, CultureInfo.InvariantCulture, out v);
                    }
                }
                return new Handle(v);
            }
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public void UpgradeOpen() { }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public void DowngradeOpen() { }

        public virtual ResultBuffer XData
        {
            get
            {
                if (string.IsNullOrEmpty(ObjectId.HandleHex)) return null;
                var d = BridgeClient.Bridge("entity", new Dictionary<string, object> { { "handle", ObjectId.HandleHex } });
                if (d == null || !d.ContainsKey("xdata") || !(d["xdata"] is IEnumerable)) return null;
                var rb = new ResultBuffer();
                foreach (var item in (IEnumerable)d["xdata"])
                {
                    if (item is Dictionary<string, object>)
                    {
                        var tag = (Dictionary<string, object>)item;
                        int code = BridgeClient.GetInt(tag, "code", 1000);
                        string val = BridgeClient.GetStr(tag, "value", "");
                        rb.Add(new TypedValue(code, val));
                    }
                }
                return rb.Items.Count > 0 ? rb : null;
            }
            set
            {
                if (string.IsNullOrEmpty(ObjectId.HandleHex)) return;
                var xdataPayload = new List<Dictionary<string, object>>();
                if (value != null)
                {
                    foreach (var tv in value.Items)
                    {
                        xdataPayload.Add(new Dictionary<string, object>
                        {
                            { "code", (int)tv.TypeCode },
                            { "value", tv.Value != null ? Convert.ToString(tv.Value, CultureInfo.InvariantCulture) : "" }
                        });
                    }
                }
                BridgeClient.Bridge("set_xdata", new Dictionary<string, object>
                {
                    { "handle", ObjectId.HandleHex },
                    { "xdata", xdataPayload }
                });
            }
        }
    }

    public class Entity : DBObject
    {
    }

    public class Database
    {
        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public ObjectId GetObjectId(bool createIfNotFound, Handle objHandle, int identifier)
        {
            return new ObjectId { HandleHex = objHandle.ToString() };
        }
    }

    public class Transaction : DisposableWrapper
    {
        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public DBObject GetObject(ObjectId id, OpenMode mode)
        {
            return new Entity { ObjectId = id };
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public DBObject GetObject(ObjectId id, OpenMode mode, bool openErased)
        {
            return new Entity { ObjectId = id };
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public DBObject GetObject(ObjectId id, OpenMode mode, bool openErased, bool forceOpenOnLockedLayer)
        {
            return new Entity { ObjectId = id };
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public void Commit() { }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public void Abort() { }
    }

    public class TransactionManager
    {
        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public Transaction StartTransaction()
        {
            return new Transaction();
        }
    }
}

namespace Autodesk.AutoCAD.EditorInput
{
    using Autodesk.AutoCAD.DatabaseServices;
    using CadCraft.NetHost;

    public enum PromptStatus
    {
        Cancel = -5002,
        Error = -5001,
        None = 5000,
        OK = 5100
    }

    public class SelectionFilter
    {
        private readonly TypedValue[] _filter;

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public SelectionFilter(TypedValue[] value)
        {
            _filter = value ?? new TypedValue[0];
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public TypedValue[] GetFilter()
        {
            return _filter;
        }
    }

    public class PromptSelectionOptions
    {
        public string MessageForAdding { get; set; }
        public string MessageForRemoval { get; set; }
        public bool SingleOnly { get; set; }
        public bool SinglePickInSpace { get; set; }
        public bool RejectObjectsOnLockedLayers { get; set; }
    }

    public class SelectedObject
    {
        private readonly ObjectId _id;

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public SelectedObject(ObjectId id)
        {
            _id = id;
        }

        public ObjectId ObjectId
        {
            get { return _id; }
        }
    }

    public class SelectionSet : IEnumerable
    {
        private readonly ObjectId[] _ids;

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public SelectionSet(ObjectId[] ids)
        {
            _ids = ids ?? new ObjectId[0];
        }

        public int Count
        {
            get { return _ids.Length; }
        }

        public SelectedObject this[int index]
        {
            get { return new SelectedObject(_ids[index]); }
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public ObjectId[] GetObjectIds()
        {
            return _ids;
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public IEnumerator GetEnumerator()
        {
            var list = new List<SelectedObject>(_ids.Length);
            for (int i = 0; i < _ids.Length; i++)
            {
                list.Add(new SelectedObject(_ids[i]));
            }
            return list.GetEnumerator();
        }
    }

    public class PromptSelectionResult
    {
        private readonly PromptStatus _status;
        private readonly SelectionSet _value;

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public PromptSelectionResult(PromptStatus status, SelectionSet value)
        {
            _status = status;
            _value = value;
        }

        public PromptStatus Status
        {
            get { return _status; }
        }

        public SelectionSet Value
        {
            get { return _value; }
        }
    }

    public class Editor
    {
        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public PromptSelectionResult SelectAll()
        {
            return SelectAll(null);
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public PromptSelectionResult SelectAll(SelectionFilter filter)
        {
            string appFilter = "";
            if (filter != null && filter.GetFilter() != null)
            {
                foreach (var tv in filter.GetFilter())
                {
                    if (tv.TypeCode == 1001 && tv.Value != null)
                    {
                        appFilter = Convert.ToString(tv.Value);
                    }
                }
            }
            var res = BridgeClient.Bridge("select_all", new Dictionary<string, object> { { "app", appFilter } });
            var ids = new List<ObjectId>();
            if (res != null && res.ContainsKey("entities") && res["entities"] is IEnumerable)
            {
                foreach (var item in (IEnumerable)res["entities"])
                {
                    if (item is Dictionary<string, object>)
                    {
                        string h = BridgeClient.GetStr((Dictionary<string, object>)item, "handle", "");
                        if (!string.IsNullOrEmpty(h))
                        {
                            ids.Add(new ObjectId { HandleHex = h });
                        }
                    }
                }
            }
            if (ids.Count > 0)
            {
                return new PromptSelectionResult(PromptStatus.OK, new SelectionSet(ids.ToArray()));
            }
            return new PromptSelectionResult(PromptStatus.Error, new SelectionSet(new ObjectId[0]));
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public PromptSelectionResult GetSelection()
        {
            return GetSelection(null, null);
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public PromptSelectionResult GetSelection(PromptSelectionOptions options)
        {
            return GetSelection(options, null);
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public PromptSelectionResult GetSelection(PromptSelectionOptions options, SelectionFilter filter)
        {
            string prompt = (options != null && !string.IsNullOrEmpty(options.MessageForAdding))
                ? options.MessageForAdding
                : "Select objects in drawing (press Enter to confirm)";
            var pick = BridgeClient.InteractivePick("selection", prompt);
            var ids = new List<ObjectId>();
            if (pick != null && pick.ContainsKey("handles") && pick["handles"] is IEnumerable)
            {
                foreach (var hObj in (IEnumerable)pick["handles"])
                {
                    string h = Convert.ToString(hObj);
                    if (!string.IsNullOrEmpty(h))
                    {
                        ids.Add(new ObjectId { HandleHex = h });
                    }
                }
            }
            if (ids.Count > 0)
            {
                return new PromptSelectionResult(PromptStatus.OK, new SelectionSet(ids.ToArray()));
            }
            return new PromptSelectionResult(PromptStatus.Cancel, new SelectionSet(new ObjectId[0]));
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public PromptSelectionResult SelectImplied()
        {
            var st = BridgeClient.Bridge("state");
            var ids = new List<ObjectId>();
            if (st != null && st.ContainsKey("selection") && st["selection"] is IEnumerable)
            {
                foreach (var h in (IEnumerable)st["selection"])
                {
                    string hex = Convert.ToString(h);
                    if (!string.IsNullOrEmpty(hex))
                    {
                        ids.Add(new ObjectId { HandleHex = hex });
                    }
                }
            }
            if (ids.Count > 0)
            {
                return new PromptSelectionResult(PromptStatus.OK, new SelectionSet(ids.ToArray()));
            }
            return new PromptSelectionResult(PromptStatus.Error, new SelectionSet(new ObjectId[0]));
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public void SetImpliedSelection(ObjectId[] selectedObjects)
        {
            var handles = new List<string>();
            if (selectedObjects != null)
            {
                foreach (var id in selectedObjects)
                {
                    if (!string.IsNullOrEmpty(id.HandleHex))
                    {
                        handles.Add(id.HandleHex);
                    }
                }
            }
            BridgeClient.Bridge("set_selection", new Dictionary<string, object> { { "handles", handles } });
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public void SetImpliedSelection(Entity[] selectedObjects)
        {
            BridgeClient.Bridge("set_selection", new Dictionary<string, object> { { "handles", new string[0] } });
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public void WriteMessage(string message, params object[] parameter)
        {
            string formatted = (parameter != null && parameter.Length > 0) ? string.Format(message, parameter) : message;
            var docProxy = Autodesk.AutoCAD.ApplicationServices.Application.DocumentManager.MdiActiveDocument.AcadDocument;
            var rp = RemotingServices.GetRealProxy(docProxy) as AcadDocumentProxy;
            if (rp != null)
            {
                BridgeClient.Call("command", new Dictionary<string, object>
                {
                    { "id", "lisp" },
                    { "params", new Dictionary<string, object> { { "code", "(princ \"" + (formatted ?? "").Replace("\\", "\\\\").Replace("\"", "\\\"") + "\") (princ)" } } }
                });
            }
        }
    }
}

namespace Autodesk.AutoCAD.ApplicationServices
{
    using Autodesk.AutoCAD.DatabaseServices;
    using Autodesk.AutoCAD.EditorInput;
    using CadCraft.NetHost;

    public class TransactionManager : Autodesk.AutoCAD.DatabaseServices.TransactionManager
    {
        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public new Transaction StartTransaction()
        {
            return new Transaction();
        }
    }

    public class DocumentLock : IDisposable
    {
        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public void Dispose() { }
    }

    public class Document
    {
        private readonly object _acadDoc = new AcadDocumentProxy().GetTransparentProxy();
        private readonly Editor _editor = new Editor();
        private readonly Database _db = new Database();
        private readonly TransactionManager _tm = new TransactionManager();

        public object AcadDocument
        {
            get { return _acadDoc; }
        }

        public Editor Editor
        {
            get { return _editor; }
        }

        public Database Database
        {
            get { return _db; }
        }

        public TransactionManager TransactionManager
        {
            get { return _tm; }
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public DocumentLock LockDocument()
        {
            return new DocumentLock();
        }
    }

    public static class DocumentExtension
    {
        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public static object GetAcadDocument(Document doc)
        {
            return doc != null ? doc.AcadDocument : Application.DocumentManager.MdiActiveDocument.AcadDocument;
        }
    }

    public class DocumentCollection
    {
        private readonly Document _active = new Document();

        public Document MdiActiveDocument
        {
            get { return _active; }
        }
    }

    public static class Application
    {
        private static readonly DocumentCollection _docMgr = new DocumentCollection();
        private static readonly Autodesk.AutoCAD.Windows.Window _mainWin = new Autodesk.AutoCAD.Windows.Window();

        public static DocumentCollection DocumentManager
        {
            get { return _docMgr; }
        }

        public static object AcadApplication
        {
            get { return _docMgr.MdiActiveDocument.AcadDocument; }
        }

        public static Autodesk.AutoCAD.Windows.Window MainWindow
        {
            get { return _mainWin; }
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public static object GetSystemVariable(string name)
        {
            var doc = (Autodesk.AutoCAD.Interop.AcadDocument)_docMgr.MdiActiveDocument.AcadDocument;
            return doc.GetVariable(name);
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public static void SetSystemVariable(string name, object value)
        {
            var doc = (Autodesk.AutoCAD.Interop.AcadDocument)_docMgr.MdiActiveDocument.AcadDocument;
            doc.SetVariable(name, value);
        }
    }
}

namespace Autodesk.AutoCAD.ApplicationServices.Core
{
    public static class Application
    {
        public static Autodesk.AutoCAD.ApplicationServices.DocumentCollection DocumentManager
        {
            get { return Autodesk.AutoCAD.ApplicationServices.Application.DocumentManager; }
        }

        public static Autodesk.AutoCAD.Windows.Window MainWindow
        {
            get { return Autodesk.AutoCAD.ApplicationServices.Application.MainWindow; }
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public static object GetSystemVariable(string name)
        {
            return Autodesk.AutoCAD.ApplicationServices.Application.GetSystemVariable(name);
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public static void SetSystemVariable(string name, object value)
        {
            Autodesk.AutoCAD.ApplicationServices.Application.SetSystemVariable(name, value);
        }
    }
}

namespace Autodesk.AutoCAD.Windows
{
    public class Window
    {
        public enum State
        {
            Normal = 0,
            Minimized = 1,
            Maximized = 2
        }

        public State WindowState { get; set; }
    }

    [Flags]
    public enum DockSides
    {
        None = 0,
        Left = 4096,
        Top = 8192,
        Right = 16384,
        Bottom = 32768
    }

    public enum PaletteSetStyles
    {
        NameEditable = 1,
        ShowPropertiesMenu = 2,
        ShowAutoHideButton = 4,
        ShowCloseButton = 8,
        SingleRowDock = 16,
        Notify = 32,
        Snappable = 64,
        ShowTabForSingle = 128,
        UsePaletteNameAsTitleForSingle = 256
    }

    public class Palette
    {
        public string Name { get; set; }
    }

    public class PaletteSet
    {
        private readonly Form _hostForm;
        private readonly TabControl _tabs;
        private Size _size = new Size(340, 620);

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public PaletteSet(string name) : this(name, Guid.Empty) { }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public PaletteSet(string name, Guid toolID) : this(name, "", toolID) { }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public PaletteSet(string name, string cmd, Guid toolID)
        {
            _hostForm = new Form
            {
                Text = string.IsNullOrEmpty(name) ? "CADCraft Palette" : name,
                Size = _size,
                StartPosition = FormStartPosition.Manual,
                Location = new Point(60, 90),
                TopMost = true,
                FormBorderStyle = FormBorderStyle.SizableToolWindow,
                ShowInTaskbar = true
            };
            _tabs = new TabControl
            {
                Dock = DockStyle.Fill
            };
            _hostForm.Controls.Add(_tabs);
            _hostForm.FormClosing += (s, e) =>
            {
                if (e.CloseReason == CloseReason.UserClosing)
                {
                    e.Cancel = true;
                    _hostForm.Hide();
                }
            };
        }

        public PaletteSetStyles Style { get; set; }
        public DockSides DockEnabled { get; set; }
        public DockSides Dock { get; set; }
        public Size MinimumSize
        {
            get { return _hostForm.MinimumSize; }
            set { _hostForm.MinimumSize = value; }
        }

        public Size Size
        {
            get { return _hostForm.Size; }
            set
            {
                _size = value;
                _hostForm.Size = value;
            }
        }

        public bool KeepFocus { get; set; }

        public bool Visible
        {
            get { return _hostForm.Visible; }
            set
            {
                if (value)
                {
                    _hostForm.Show();
                    _hostForm.BringToFront();
                }
                else
                {
                    _hostForm.Hide();
                }
            }
        }

        /// <summary>AutoCAD .NET / COM interop member.</summary>
        public Palette Add(string name, Control control)
        {
            var page = new TabPage(name ?? "Palette");
            if (control != null)
            {
                if (control is Form)
                {
                    var f = (Form)control;
                    f.TopLevel = false;
                    f.FormBorderStyle = FormBorderStyle.None;
                    f.Dock = DockStyle.Fill;
                    f.Visible = true;
                }
                else
                {
                    control.Dock = DockStyle.Fill;
                }
                page.Controls.Add(control);
            }
            _tabs.TabPages.Add(page);
            return new Palette { Name = name };
        }
    }
}
